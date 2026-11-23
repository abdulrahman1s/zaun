//! E2E params with a version range, as put in connection links: binary and `v=..&x3dh=..&kem_key=..` forms (Ratchet.hs `E2ERatchetParamsUri`; nook ratchet/params.ts).
use super::error::{RatchetError, Result};
use super::kem::{KemCiphertext, KemPublicKey};
use super::kem_params::KemParams;
use super::params::{E2eParams, RcvParams, require_x448};
use super::query::parse_query;
use super::version::adjust_e2e_range;
use crate::crypto::PublicKey;
use crate::encoding::{
    Decode, DecodeError, Decoder, Encode, EncodeError, Version, VersionRange, b64_decode,
    b64_encode,
};

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ParamsUri {
    pub vr: VersionRange,
    pub k1: PublicKey,
    pub k2: PublicKey,
    pub kem: Option<KemParams>,
}

impl ParamsUri {
    #[must_use]
    pub fn from_params(p: &RcvParams, vr: VersionRange) -> Self {
        let p = p.params();
        Self {
            vr,
            k1: p.k1.clone(),
            k2: p.k2.clone(),
            kem: p.kem.clone(),
        }
    }

    /// The params of one version out of the range (`toVersionT`); only proposals are valid in a link.
    pub fn to_rcv_params(&self, v: Version) -> Result<RcvParams> {
        let p = E2eParams::new(v, self.k1.clone(), self.k2.clone(), self.kem.clone())?;
        RcvParams::try_from(p)
    }

    /// `strEncode`: the query string without escaping.
    #[must_use]
    pub fn to_query(&self) -> String {
        let (min, max) = (self.vr.min().0, self.vr.max().0);
        let v = if min == max {
            min.to_string()
        } else {
            format!("{min}-{max}")
        };
        let keys = [&self.k1, &self.k2].map(|k| b64_encode(&k.to_der(), true));
        let mut s = format!("v={v}&x3dh={}", keys.join(","));
        match &self.kem {
            None => {}
            Some(KemParams::Proposed(key)) => s.push_str(&kem_key(key)),
            Some(KemParams::Accepted { ct, key }) => {
                s.push_str(&format!("&kem_ct={}", b64_encode(ct.as_ref(), true)));
                s.push_str(&kem_key(key));
            }
        }
        s
    }

    /// `strP`, with `adjustE2EVRange`.
    pub fn from_query(q: &str) -> Result<Self> {
        let params = parse_query(q);
        let get = |k: &str| params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        let vr = get("v").ok_or(DecodeError::Invalid("no qs param v"))?;
        let vr = adjust_e2e_range(parse_range(vr)?);
        let x3dh = get("x3dh").ok_or(DecodeError::Invalid("no qs param x3dh"))?;
        let [k1, k2] = x3dh.split(',').collect::<Vec<_>>()[..] else {
            return Err(DecodeError::Invalid("bad e2e params").into());
        };
        let key = |s: &str| -> Result<PublicKey> {
            let k = PublicKey::from_der(&b64_decode(s)?)
                .map_err(|_| DecodeError::Invalid("e2e key"))?;
            require_x448(&k)?;
            Ok(k)
        };
        let kem = match (get("kem_key"), get("kem_ct")) {
            (None, None) => None,
            (None, Some(_)) => return Err(DecodeError::Invalid("kem_ct without kem_key").into()),
            (Some(k), None) => Some(KemParams::Proposed(KemPublicKey::from_bytes(&b64_decode(
                k,
            )?)?)),
            (Some(k), Some(ct)) => Some(KemParams::Accepted {
                ct: KemCiphertext::from_bytes(&b64_decode(ct)?)?,
                key: KemPublicKey::from_bytes(&b64_decode(k)?)?,
            }),
        };
        Ok(Self {
            vr,
            k1: key(k1)?,
            k2: key(k2)?,
            kem,
        })
    }
}

fn kem_key(key: &KemPublicKey) -> String {
    format!("&kem_key={}", b64_encode(key.as_ref(), true))
}

fn parse_range(s: &str) -> Result<VersionRange> {
    let bad = || RatchetError::from(DecodeError::Invalid("version range"));
    let num = |p: &str| p.parse::<u16>().map(Version).map_err(|_| bad());
    let (min, max) = match s.split_once('-') {
        Some((a, b)) => (num(a)?, num(b)?),
        None => (num(s)?, num(s)?),
    };
    VersionRange::new(min, max).ok_or_else(bad)
}

impl Encode for ParamsUri {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        (self.vr, &self.k1, &self.k2, &self.kem).encode(out)
    }
}

impl Decode for ParamsUri {
    fn decode(d: &mut Decoder<'_>) -> Result<Self, DecodeError> {
        let (vr, k1, k2, kem) = <_>::decode(d)?;
        require_x448(&k1)?;
        require_x448(&k2)?;
        Ok(Self { vr, k1, k2, kem })
    }
}
