//! Encrypting a message: advance the sending chain, encrypt the header, then the padded body (Ratchet.hs `rcEncryptHeader`, `rcEncryptMsg`, `rcCheckCanPad`).
use super::envelope::{EncMessageHeader, EncRatchetMessage};
use super::error::{RatchetError, Result};
use super::header::MsgHeader;
use super::kdf::{MessageKey, chain_kdf};
use super::kem_state::RatchetKem;
use super::lengths::padded_header_len;
use super::pq::PqEncryption;
use super::state::Ratchet;
use super::version::{CURRENT_E2E_VERSION, MIN_SUPPORTED_E2E_VERSION};
use crate::crypto::{aead_encrypt, can_pad};
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError, Large, Version};

/// What `encrypt_header` hands over for `encrypt_msg`, so the message can be encrypted later (and kept in the store until then).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MsgEncryptKey {
    version: Version,
    key: MessageKey,
    ad: Vec<u8>,
    enc_header: Vec<u8>,
}

impl Ratchet {
    /// `rcEncryptHeader`. `pq` turns the KEM on or off from this message; `None` keeps it. `supported` is the highest ratchet version we speak.
    pub fn encrypt_header(
        &mut self,
        pq: Option<PqEncryption>,
        supported: Version,
    ) -> Result<MsgEncryptKey> {
        let snd = self.snd.as_ref().ok_or(RatchetError::State)?;
        let step = chain_kdf(&snd.cks)?;
        let v = self.versions.current.max(MIN_SUPPORTED_E2E_VERSION);
        let enable = pq.unwrap_or(self.enable_kem);
        // Support, and so the large header, is only ever turned on.
        let support = self.support_kem.enable_with(enable);
        // Peers upgrade their current version when they decrypt this.
        let max_supported = supported.max(if pq == Some(PqEncryption::ON) {
            MIN_SUPPORTED_E2E_VERSION
        } else {
            v
        });
        let header = MsgHeader {
            max_version: max_supported,
            dhrs: self.dhrs.public()?,
            kem: self.kem.as_ref().map(RatchetKem::header_params),
            pn: self.pn,
            ns: self.ns,
        };
        let (body, tag) = aead_encrypt(
            &snd.hks.aes(),
            &step.header_iv,
            padded_header_len(support),
            &self.ad,
            &header.to_bytes()?,
        )?;
        let enc_header = EncMessageHeader {
            version: v,
            iv: step.header_iv,
            tag,
            body,
            legacy: false,
        }
        .to_bytes()?;
        let ns = self
            .ns
            .checked_add(1)
            .ok_or(RatchetError::BadState("message counter"))?;
        let key = MsgEncryptKey {
            version: v,
            key: step.message,
            ad: self.ad.clone(),
            enc_header,
        };
        if let Some(s) = self.snd.as_mut() {
            s.cks = step.next;
        }
        self.ns = ns;
        self.support_kem = support;
        self.enable_kem = enable;
        self.versions.max_supported = max_supported;
        if let (Some(PqEncryption::OFF), Some(k)) = (pq, self.kem.as_mut()) {
            k.accepted = None;
        }
        Ok(key)
    }

    /// Header and body in one step; `padded_len` is the size of the plaintext after padding.
    pub fn encrypt(
        &mut self,
        msg: &[u8],
        padded_len: usize,
        pq: Option<PqEncryption>,
    ) -> Result<Vec<u8>> {
        if !can_pad(msg.len(), padded_len) {
            return Err(RatchetError::LargeMsg);
        }
        let key = self.encrypt_header(pq, CURRENT_E2E_VERSION)?;
        key.encrypt_msg(padded_len, msg)
    }
}

impl MsgEncryptKey {
    /// `rcEncryptMsg`.
    pub fn encrypt_msg(&self, padded_len: usize, msg: &[u8]) -> Result<Vec<u8>> {
        let ad = [self.ad.as_slice(), &self.enc_header].concat();
        let (body, tag) = aead_encrypt(&self.key.key.aes(), &self.key.iv, padded_len, &ad, msg)?;
        let wire = EncRatchetMessage {
            header: self.enc_header.clone(),
            tag,
            body,
        };
        Ok(wire.to_bytes()?)
    }

    #[must_use]
    pub fn version(&self) -> Version {
        self.version
    }
}

impl Encode for MsgEncryptKey {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (
            self.version,
            &self.ad,
            self.key.key.as_bytes(),
            &self.key.iv,
        )
            .encode(out)?;
        Large(self.enc_header.clone()).encode(out)
    }
}

impl Decode for MsgEncryptKey {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (version, ad, key, iv) = <(Version, Vec<u8>, Vec<u8>, Vec<u8>)>::decode(d)?;
        let bad = || DecodeError::Invalid("message key");
        Ok(Self {
            version,
            ad,
            key: MessageKey {
                key: super::key::Key32::new(key.try_into().map_err(|_| bad())?),
                iv: iv.try_into().map_err(|_| bad())?,
            },
            enc_header: Large::decode(d)?.0,
        })
    }
}
