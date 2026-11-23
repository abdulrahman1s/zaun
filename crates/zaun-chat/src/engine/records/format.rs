//! How engine records are kept: JSON with a format version, in the store's namespace "chat".

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::store::StoreError;

pub const NS: &str = "chat";

/// The record format. A record of another version is refused, never guessed at.
pub const VERSION: u32 = 1;

#[derive(Serialize)]
struct Out<'a, T> {
    v: u32,
    r: &'a T,
}

#[derive(Deserialize)]
struct In<T> {
    v: u32,
    r: T,
}

pub fn encode<T: Serialize>(record: &T) -> Result<Vec<u8>, StoreError> {
    serde_json::to_vec(&Out {
        v: VERSION,
        r: record,
    })
    .map_err(|e| StoreError::Corrupt(format!("record encode: {e}")))
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, StoreError> {
    let rec: In<T> = serde_json::from_slice(bytes)
        .map_err(|e| StoreError::Corrupt(format!("record decode: {e}")))?;
    if rec.v != VERSION {
        return Err(StoreError::Corrupt(format!("record version {}", rec.v)));
    }
    Ok(rec.r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_of_another_version_is_refused() {
        assert!(decode::<u32>(br#"{"v":2,"r":7}"#).is_err());
        assert_eq!(
            decode::<u32>(&encode(&7u32).unwrap_or_default()).ok(),
            Some(7)
        );
    }
}
