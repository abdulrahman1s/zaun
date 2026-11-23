//! Field access on a JSON object the way aeson's `.:` and `.:?` read it (null counts as absent for `opt`).
use super::error::{ChatError, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

pub(crate) fn req<T: DeserializeOwned>(o: &Map<String, Value>, key: &str) -> Result<T> {
    let v = o
        .get(key)
        .ok_or_else(|| ChatError::Parse(format!("key \"{key}\" not found")))?;
    serde_json::from_value(v.clone()).map_err(|e| ChatError::Parse(format!("\"{key}\": {e}")))
}

pub(crate) fn opt<T: DeserializeOwned>(o: &Map<String, Value>, key: &str) -> Result<Option<T>> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => serde_json::from_value(v.clone())
            .map(Some)
            .map_err(|e| ChatError::Parse(format!("\"{key}\": {e}"))),
    }
}

/// Our own types always serialize; `Null` is unreachable and only keeps this total.
pub(crate) fn val<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Builds `params`: required keys first, optional ones only when present.
#[derive(Default)]
pub(crate) struct Params(pub Map<String, Value>);

impl Params {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put<T: Serialize>(mut self, key: &str, v: &T) -> Self {
        self.0.insert(key.to_owned(), val(v));
        self
    }

    pub fn put_opt<T: Serialize>(self, key: &str, v: &Option<T>) -> Self {
        match v {
            Some(v) => self.put(key, v),
            None => self,
        }
    }

    pub fn put_if(self, key: &str, on: bool) -> Self {
        if on { self.put(key, &true) } else { self }
    }

    pub fn done(self) -> Map<String, Value> {
        self.0
    }
}
