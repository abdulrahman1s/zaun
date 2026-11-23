//! What the store can fail with; the helper maps `WrongKey` to exit code 3.

use std::fmt;

#[derive(Debug)]
pub enum StoreError {
    Open(String),
    WrongKey,
    Io(std::io::Error),
    Corrupt(String),
    Sql(String),
    /// The caller passed something the store cannot keep (a bad namespace, a missing row).
    Invalid(&'static str),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Open(e) => write!(f, "cannot open the chat store: {e}"),
            StoreError::WrongKey => f.write_str("the chat store key is wrong"),
            StoreError::Io(e) => write!(f, "chat store I/O: {e}"),
            StoreError::Corrupt(e) => write!(f, "the chat store is damaged: {e}"),
            StoreError::Sql(e) => write!(f, "chat store query failed: {e}"),
            StoreError::Invalid(e) => write!(f, "invalid chat store request: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

impl From<ormlite::SqlxError> for StoreError {
    fn from(e: ormlite::SqlxError) -> Self {
        use ormlite::SqlxError as E;
        match e {
            E::Io(io) => StoreError::Io(io),
            E::Database(db) => {
                let code: i64 = db.code().and_then(|c| c.parse().ok()).unwrap_or(0);
                match code & 0xff {
                    11 | 26 => StoreError::Corrupt(db.message().to_owned()),
                    _ => StoreError::Sql(db.message().to_owned()),
                }
            }
            other => StoreError::Sql(other.to_string()),
        }
    }
}
