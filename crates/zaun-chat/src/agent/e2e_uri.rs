//! Ratchet parameters as the `e2e` query of a link (Crypto/Ratchet.hs `E2ERatchetParamsUri` `StrEncoding`, AGPL-3.0-only).
use crate::agent::e2e::E2eParamsUri;
use crate::agent::error::{AgentError, Result};
use crate::agent::kem::KemParams;
use crate::agent::keys::{key_str, parse_key_str};
use crate::agent::query::{self, pair};
use crate::agent::version::{MIN_SUPPORTED_E2E, parse_range, raise_range, range_str};
use crate::crypto::Alg;
use crate::encoding::{b64_decode, b64_encode};

impl E2eParamsUri {
    /// `strEncode`: the unescaped query `v=3&x3dh=<k1>,<k2>[&kem_ct=…]&kem_key=…`.
    #[must_use]
    pub fn to_query(&self) -> String {
        let mut q = vec![
            pair("v", range_str(&self.v_range)),
            pair(
                "x3dh",
                format!("{},{}", key_str(&self.key1), key_str(&self.key2)),
            ),
        ];
        let kem = |b: &[u8]| b64_encode(b, true);
        match &self.kem {
            Some(KemParams::Proposed { key }) => q.push(pair("kem_key", kem(key))),
            Some(KemParams::Accepted { ct, key }) => {
                q.push(pair("kem_ct", kem(ct)));
                q.push(pair("kem_key", kem(key)));
            }
            None => {}
        }
        query::render_raw(&q)
    }

    pub fn parse(s: &str) -> Result<Self> {
        let q = query::parse(s);
        let v = query::get(&q, "v").ok_or(AgentError::MissingParam("v"))?;
        let v_range = raise_range(parse_range(v)?, MIN_SUPPORTED_E2E);
        let x3dh = query::get(&q, "x3dh").ok_or(AgentError::MissingParam("x3dh"))?;
        let [k1, k2] = x3dh.split(',').collect::<Vec<_>>()[..] else {
            return Err(AgentError::Parse("e2e params"));
        };
        let kem = match (query::get(&q, "kem_key"), query::get(&q, "kem_ct")) {
            (Some(key), None) => Some(KemParams::Proposed {
                key: b64_decode(key)?,
            }),
            (Some(key), Some(ct)) => Some(KemParams::Accepted {
                ct: b64_decode(ct)?,
                key: b64_decode(key)?,
            }),
            (None, None) => None,
            (None, Some(_)) => return Err(AgentError::Parse("kem_ct without kem_key")),
        };
        Ok(Self {
            v_range,
            key1: parse_key_str(k1, Alg::X448)?,
            key2: parse_key_str(k2, Alg::X448)?,
            kem,
        })
    }
}
