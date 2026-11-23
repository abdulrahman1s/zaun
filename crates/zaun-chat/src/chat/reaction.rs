//! Message reactions (Protocol.hs `MsgReaction`, `MREmojiChar`).
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `{"type":"emoji","emoji":"👍"}`; any other reaction keeps its JSON and shows as `?`.
#[derive(Clone, Debug, PartialEq)]
pub enum MsgReaction {
    Emoji(char),
    Unknown(Value),
}

/// The reaction whitelist, matched per `Char` (so `❤` is U+2764 without the variation selector).
pub const REACTION_EMOJI: &str = "👍👎😀😂😢❤\u{fe0f}🚀✅";

impl Serialize for MsgReaction {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Emoji(c) => {
                serde_json::json!({"type": "emoji", "emoji": c.to_string()}).serialize(s)
            }
            Self::Unknown(v) => v.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for MsgReaction {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let Some(o) = v.as_object() else {
            return Err(serde::de::Error::custom(
                "bad MsgReaction, expected an object",
            ));
        };
        let Some(tag) = o.get("type").and_then(Value::as_str) else {
            return Err(serde::de::Error::custom(
                "bad MsgReaction, \"type\" not found",
            ));
        };
        let emoji = match (tag, o.get("emoji").and_then(Value::as_str)) {
            ("emoji", Some(e)) => single_char(e).filter(|c| REACTION_EMOJI.contains(*c)),
            _ => None,
        };
        Ok(emoji.map_or(Self::Unknown(v), Self::Emoji))
    }
}

fn single_char(s: &str) -> Option<char> {
    let mut it = s.chars();
    it.next().filter(|_| it.next().is_none())
}
