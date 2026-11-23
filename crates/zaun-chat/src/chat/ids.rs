//! Ids that travel as padded base64url JSON strings, minted as 7.1.4 does (Types.hs `SharedMsgId`, Store/Shared.hs `encodedRandomBytes`).
use super::error::{ChatError, Result};
use base64::Engine;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

const URL: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// Padded base64url, the `strEncode` of a `ByteString`.
pub fn b64_encode(bytes: &[u8]) -> String {
    URL.encode(bytes)
}

/// Accepts padded and unpadded input; the reference's own ids are padded.
pub fn b64_decode(s: &str) -> Result<Vec<u8>> {
    URL.decode(s)
        .map_err(|e| ChatError::Parse(format!("bad base64url: {e}")))
}

macro_rules! byte_id {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, PartialEq, Eq, Hash, Debug)]
        pub struct $name(pub Vec<u8>);

        impl $name {
            pub fn as_bytes(&self) -> &[u8] {
                &self.0
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&b64_encode(&self.0))
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                b64_decode(&s).map(Self).map_err(serde::de::Error::custom)
            }
        }
    };
}

byte_id!(
    /// Message id; compare and echo the decoded bytes, never the text.
    SharedMsgId
);
byte_id!(MemberId);
byte_id!(
    /// 16 raw random bytes, not double-encoded.
    CallId
);
byte_id!(Probe);
byte_id!(ProbeHash);
byte_id!(XContactId);
byte_id!(GroupLinkId);

/// `encodedRandomBytes n`: the standard base64 *text* of `n` random bytes, as bytes (the quirk:
/// JSON then encodes those 4n/3 characters once more).
pub fn encoded_random_bytes(n: usize) -> Result<Vec<u8>> {
    let mut raw = vec![0u8; n];
    getrandom::fill(&mut raw).map_err(|_| ChatError::Random)?;
    Ok(base64::engine::general_purpose::STANDARD
        .encode(raw)
        .into_bytes())
}

impl SharedMsgId {
    pub fn mint() -> Result<Self> {
        encoded_random_bytes(12).map(Self)
    }
}

impl MemberId {
    pub fn mint() -> Result<Self> {
        encoded_random_bytes(12).map(Self)
    }
}

impl CallId {
    pub fn mint() -> Result<Self> {
        let mut raw = vec![0u8; 16];
        getrandom::fill(&mut raw).map_err(|_| ChatError::Random)?;
        Ok(Self(raw))
    }
}
