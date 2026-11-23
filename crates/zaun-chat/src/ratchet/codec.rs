//! The stored form of a ratchet: a format byte, then every field, skipped keys last. The engine keeps these bytes in the store, so a new format gets a new first byte.
use super::key::Key32;
use super::state::{Ratchet, RcvRatchet, SndRatchet};
use super::version::RatchetVersions;
use crate::encoding::{Decode, DecodeError, Decoder, Encode, EncodeError};

const FORMAT: u8 = 1;

impl Encode for SndRatchet {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (&self.dhr, &self.cks, &self.hks).encode(out)
    }
}

impl Decode for SndRatchet {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (dhr, cks, hks) = <_>::decode(d)?;
        Ok(Self { dhr, cks, hks })
    }
}

impl Encode for RcvRatchet {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (&self.ckr, &self.hkr).encode(out)
    }
}

impl Decode for RcvRatchet {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (ckr, hkr) = <_>::decode(d)?;
        Ok(Self { ckr, hkr })
    }
}

impl Encode for Ratchet {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        FORMAT.encode(out)?;
        (
            self.versions.current,
            self.versions.max_supported,
            &self.ad,
            &self.vc_pq,
        )
            .encode(out)?;
        (&self.dhrs, &self.kem).encode(out)?;
        (
            self.support_kem,
            self.enable_kem,
            self.snd_kem,
            self.rcv_kem,
        )
            .encode(out)?;
        (&self.rk, &self.snd, &self.rcv).encode(out)?;
        (self.ns, self.nr, self.pn, &self.nhks, &self.nhkr).encode(out)?;
        self.skipped.encode(out)
    }
}

impl Decode for Ratchet {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        if u8::decode(d)? != FORMAT {
            return Err(DecodeError::Invalid("ratchet state format"));
        }
        let (current, max_supported, ad, vc_pq) = <(_, _, Vec<u8>, Option<Key32>)>::decode(d)?;
        let (dhrs, kem) = <_>::decode(d)?;
        let (support_kem, enable_kem, snd_kem, rcv_kem) = <_>::decode(d)?;
        let (rk, snd, rcv) = <_>::decode(d)?;
        let (ns, nr, pn, nhks, nhkr) = <_>::decode(d)?;
        Ok(Self {
            versions: RatchetVersions {
                current,
                max_supported,
            },
            ad,
            vc_pq,
            dhrs,
            kem,
            support_kem,
            enable_kem,
            snd_kem,
            rcv_kem,
            rk,
            snd,
            rcv,
            ns,
            nr,
            pn,
            nhks,
            nhkr,
            skipped: <_>::decode(d)?,
        })
    }
}
