//! The line format: one JSON object per line; a [`Hello`] first, then `{"req":…}` in and
//! `{"ev":…}` out.

use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::event::Event;
use crate::request::Request;

/// The first line core writes: how to open the database.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub v: u32,
    pub dir: PathBuf,
    pub key: String,
    pub name: String,
    /// Relay addresses; empty = the protocol's preset relays.
    pub relays: Vec<String>,
}

impl Hello {
    #[must_use]
    pub fn new(dir: PathBuf, key: String, name: String, relays: Vec<String>) -> Self {
        Self {
            v: crate::VERSION,
            dir,
            key,
            name,
            relays,
        }
    }
}

/// Every line into the helper after the hello.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReqLine {
    pub req: Request,
}

/// Every line out of the helper.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvLine {
    pub ev: Event,
}

#[derive(Debug)]
pub enum WireError {
    /// The line is not valid JSON of the expected shape (including an unknown variant).
    Malformed(serde_json::Error),
}

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(e) => write!(f, "malformed line: {e}"),
        }
    }
}

impl std::error::Error for WireError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Malformed(e) => Some(e),
        }
    }
}

/// One line of JSON, without the trailing newline.
pub fn encode_line<T: Serialize>(value: &T) -> Result<String, WireError> {
    serde_json::to_string(value).map_err(WireError::Malformed)
}

pub fn decode_line<T: DeserializeOwned>(line: &str) -> Result<T, WireError> {
    serde_json::from_str(line.trim_end()).map_err(WireError::Malformed)
}
