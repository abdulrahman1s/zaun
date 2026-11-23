//! `ChatMessage`: the JSON envelope `{v, msgId, event, params}` (Protocol.hs `ChatMessage`, `AppMessageJson`).
use super::error::{Result, parse_err};
use super::event::{ChatEvent, decode, encode};
use super::ids::SharedMsgId;
use super::json::{opt, req};
use super::version::{CHAT_INITIAL_VRANGE, SUPPORTED_CHAT_VRANGE, VersionRange};
use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct ChatMessage {
    /// Absent on the wire means the initial range (9), see [`ChatMessage::version_range`].
    pub v: Option<VersionRange>,
    pub msg_id: Option<SharedMsgId>,
    pub event: ChatEvent,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Envelope<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    v: &'a Option<VersionRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    msg_id: &'a Option<SharedMsgId>,
    event: String,
    params: Map<String, Value>,
}

impl ChatMessage {
    /// A message as a 7.1.4 client sends it: its own version range and a freshly minted id.
    pub fn new(event: ChatEvent) -> Result<Self> {
        Ok(Self {
            v: Some(SUPPORTED_CHAT_VRANGE),
            msg_id: Some(SharedMsgId::mint()?),
            event,
        })
    }

    /// Conn-info (`x.info`, `x.contact`, ...) carries no `msgId`.
    pub fn without_id(event: ChatEvent) -> Self {
        Self {
            v: Some(SUPPORTED_CHAT_VRANGE),
            msg_id: None,
            event,
        }
    }

    pub fn version_range(&self) -> VersionRange {
        self.v.unwrap_or(CHAT_INITIAL_VRANGE)
    }

    /// `depth` counts the `x.grp.msg.forward` envelopes already entered.
    pub fn from_value(value: &Value, depth: usize) -> Result<Self> {
        let Some(o) = value.as_object() else {
            return parse_err("expected a JSON object");
        };
        let name: String = req(o, "event")?;
        let params: Map<String, Value> = req(o, "params")?;
        Ok(Self {
            v: opt(o, "v")?,
            msg_id: opt(o, "msgId")?,
            event: decode(&name, &params, depth)?,
        })
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        Self::from_value(&serde_json::from_slice(bytes)?, 0)
    }

    pub fn to_value(&self) -> Value {
        match self.envelope() {
            Ok(e) => super::json::val(&e),
            Err(_) => Value::Null,
        }
    }

    /// The JSON body; a binary `F` chunk has none.
    pub fn to_json(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&self.envelope()?)?)
    }

    fn envelope(&self) -> Result<Envelope<'_>> {
        let (event, params) = encode(&self.event)?;
        Ok(Envelope {
            v: &self.v,
            msg_id: &self.msg_id,
            event,
            params,
        })
    }
}
