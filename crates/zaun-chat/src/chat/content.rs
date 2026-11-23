//! Message content (Protocol.hs `MsgContent`, `LinkPreview`, `ReportReason`).
use super::image::ImageData;
use super::json::{opt, req};
use super::link_preview::LinkPreview;
use super::open_enum::open_enum;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};

open_enum!(ReportReason {
    Spam = "spam",
    Content = "content",
    Community = "community",
    Profile = "profile",
    Other = "other",
});

pub const UNKNOWN_MSG_TYPE: &str = "unknown message type";

#[derive(Clone, Debug, PartialEq)]
pub enum MsgContent {
    Text {
        text: String,
    },
    Link {
        text: String,
        preview: LinkPreview,
    },
    Image {
        text: String,
        image: ImageData,
    },
    Video {
        text: String,
        image: ImageData,
        duration: i64,
    },
    Voice {
        text: String,
        duration: i64,
    },
    File {
        text: String,
    },
    Report {
        text: String,
        reason: ReportReason,
    },
    /// A shared chat link; `chat_link` and `owner_sig` are kept as sent.
    Chat {
        text: String,
        chat_link: Value,
        owner_sig: Option<Value>,
    },
    Unknown {
        tag: String,
        text: String,
        json: Map<String, Value>,
    },
}

impl MsgContent {
    pub fn text(&self) -> &str {
        match self {
            Self::Text { text }
            | Self::Link { text, .. }
            | Self::Image { text, .. }
            | Self::Video { text, .. }
            | Self::Voice { text, .. }
            | Self::File { text }
            | Self::Report { text, .. }
            | Self::Chat { text, .. }
            | Self::Unknown { text, .. } => text,
        }
    }

    pub fn from_object(o: &Map<String, Value>) -> super::error::Result<Self> {
        let tag: String = req(o, "type")?;
        let text = || req::<String>(o, "text");
        Ok(match tag.as_str() {
            "text" => Self::Text { text: text()? },
            "link" => Self::Link {
                text: text()?,
                preview: req(o, "preview")?,
            },
            "image" => Self::Image {
                text: text()?,
                image: req(o, "image")?,
            },
            "video" => Self::Video {
                text: text()?,
                image: req(o, "image")?,
                duration: req(o, "duration")?,
            },
            "voice" => Self::Voice {
                text: text()?,
                duration: req(o, "duration")?,
            },
            "file" => Self::File { text: text()? },
            "report" => Self::Report {
                text: text()?,
                reason: req(o, "reason")?,
            },
            "chat" => Self::Chat {
                text: text()?,
                chat_link: req(o, "chatLink")?,
                owner_sig: opt(o, "ownerSig")?,
            },
            _ => {
                let text = opt(o, "text")?.unwrap_or_else(|| UNKNOWN_MSG_TYPE.to_owned());
                Self::Unknown {
                    tag,
                    text,
                    json: o.clone(),
                }
            }
        })
    }

    pub fn to_value(&self) -> Value {
        match self {
            Self::Text { text } => json!({"type": "text", "text": text}),
            Self::Link { text, preview } => {
                json!({"type": "link", "text": text, "preview": preview})
            }
            Self::Image { text, image } => json!({"type": "image", "text": text, "image": image}),
            Self::Video {
                text,
                image,
                duration,
            } => {
                json!({"type": "video", "text": text, "image": image, "duration": duration})
            }
            Self::Voice { text, duration } => {
                json!({"type": "voice", "text": text, "duration": duration})
            }
            Self::File { text } => json!({"type": "file", "text": text}),
            Self::Report { text, reason } => {
                json!({"type": "report", "text": text, "reason": reason})
            }
            Self::Chat {
                text,
                chat_link,
                owner_sig,
            } => {
                let mut v = json!({"type": "chat", "text": text, "chatLink": chat_link});
                if let (Some(sig), Some(o)) = (owner_sig, v.as_object_mut()) {
                    o.insert("ownerSig".to_owned(), sig.clone());
                }
                v
            }
            Self::Unknown { json, .. } => Value::Object(json.clone()),
        }
    }
}

impl Serialize for MsgContent {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_value().serialize(s)
    }
}

impl<'de> Deserialize<'de> for MsgContent {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Object(o) => Self::from_object(&o).map_err(serde::de::Error::custom),
            _ => Err(serde::de::Error::custom(
                "bad MsgContent, expected an object",
            )),
        }
    }
}
