//! Decrypting a message: try the skipped keys, then the header keys, step the ratchet, skip ahead, decrypt the body (Ratchet.hs `rcDecrypt`).
use super::envelope::{EncMessageHeader, EncRatchetMessage};
use super::error::{RatchetError, Result};
use super::header::MsgHeader;
use super::kdf::{MessageKey, chain_kdf};
use super::key::Key32;
use super::skipped::{SkippedKey, advance_chain};
use super::state::Ratchet;
use crate::crypto::aead_decrypt;
use crate::encoding::Decode;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Advance,
    Same,
}

/// The ratchet to keep, and the message or why it failed.
type Outcome = (Ratchet, Result<Vec<u8>>);

impl Ratchet {
    /// `rcDecrypt`. Errors that mean the message does not belong to this ratchet leave it as it was; a failed body, and a jump over too many messages after a step, still advance it. Persist it after every call.
    pub fn decrypt(&mut self, wire: &[u8]) -> Result<Vec<u8>> {
        let msg = EncRatchetMessage::from_bytes(wire)
            .map_err(|_| RatchetError::Header("message envelope"))?;
        let hdr = EncMessageHeader::from_bytes(&msg.header)
            .map_err(|_| RatchetError::Header("encrypted header"))?;
        let (rc, body) = self.clone().decrypt_parsed(&hdr, &msg)?;
        *self = rc;
        body
    }

    fn decrypt_parsed(
        mut self,
        hdr: &EncMessageHeader,
        msg: &EncRatchetMessage,
    ) -> Result<Outcome> {
        let (step, header) = match self.try_skipped(hdr, msg)? {
            Skipped::Done(body) => return Ok((self, body)),
            Skipped::Header(step, header) => (step, header),
            Skipped::None => self.decrypt_rc_header(hdr)?,
        };
        self.decrypt_message(step, &header, msg)
    }

    fn try_skipped(&mut self, hdr: &EncMessageHeader, msg: &EncRatchetMessage) -> Result<Skipped> {
        for hk in self.skipped.header_keys() {
            let header = match self.decrypt_header(&hk, hdr) {
                Ok(h) => h,
                Err(RatchetError::HeaderKey) => continue,
                Err(e) => return Err(e),
            };
            if let Some(mk) = self.skipped.take(&hk, header.ns) {
                return Ok(Skipped::Done(self.decrypt_body(&mk, msg)));
            }
            let step = if self.rcv.as_ref().is_some_and(|r| r.hkr == hk) {
                Step::Same
            } else if hk == self.nhkr {
                Step::Advance
            } else {
                return Err(RatchetError::HeaderKey);
            };
            return Ok(Skipped::Header(step, header));
        }
        Ok(Skipped::None)
    }

    fn decrypt_rc_header(&self, hdr: &EncMessageHeader) -> Result<(Step, MsgHeader)> {
        if let Some(rcv) = &self.rcv {
            match self.decrypt_header(&rcv.hkr, hdr) {
                Ok(h) => return Ok((Step::Same, h)),
                Err(RatchetError::HeaderKey) => {}
                Err(e) => return Err(e),
            }
        }
        Ok((Step::Advance, self.decrypt_header(&self.nhkr, hdr)?))
    }

    fn decrypt_header(&self, key: &Key32, hdr: &EncMessageHeader) -> Result<MsgHeader> {
        let bytes = aead_decrypt(&key.aes(), &hdr.iv, &self.ad, &hdr.body, &hdr.tag)
            .map_err(|_| RatchetError::HeaderKey)?;
        MsgHeader::decode_with(&bytes, hdr.legacy)
            .map_err(|_| RatchetError::Header("message header"))
    }

    fn decrypt_body(&self, mk: &MessageKey, msg: &EncRatchetMessage) -> Result<Vec<u8>> {
        let ad = [self.ad.as_slice(), &msg.header].concat();
        Ok(aead_decrypt(
            &mk.key.aes(),
            &mk.iv,
            &ad,
            &msg.body,
            &msg.tag,
        )?)
    }

    fn decrypt_message(
        mut self,
        step: Step,
        hdr: &MsgHeader,
        msg: &EncRatchetMessage,
    ) -> Result<Outcome> {
        if hdr.max_version > self.versions.current {
            self.versions.current = self
                .versions
                .current
                .max(hdr.max_version.min(self.versions.max_supported));
        }
        if step == Step::Advance {
            let skipped = self.skip_message_keys(hdr.pn)?;
            self.ratchet_step(hdr)?;
            self.skipped.add(skipped);
        }
        match self.skip_message_keys(hdr.ns) {
            Err(e) => Ok((self, Err(e))),
            Ok(skipped) => {
                self.skipped.add(skipped);
                let Some(rcv) = self.rcv.as_mut() else {
                    return Ok((self, Err(RatchetError::State)));
                };
                let ck = chain_kdf(&rcv.ckr)?;
                rcv.ckr = ck.next;
                self.nr = self.nr.saturating_add(1);
                let body = self.decrypt_body(&ck.message, msg);
                Ok((self, body))
            }
        }
    }

    /// `skipMessageKeys`: moves the receiving chain to message `until`, returning the keys it passed.
    fn skip_message_keys(&mut self, until: u32) -> Result<Vec<SkippedKey>> {
        let Some(rcv) = self.rcv.as_mut() else {
            return Ok(Vec::new());
        };
        let (ckr, nr, skipped) = advance_chain(&rcv.ckr, &rcv.hkr, self.nr, until)?;
        rcv.ckr = ckr;
        self.nr = nr;
        Ok(skipped)
    }
}

enum Skipped {
    Done(Result<Vec<u8>>),
    Header(Step, MsgHeader),
    None,
}
