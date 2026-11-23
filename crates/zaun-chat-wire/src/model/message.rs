//! One message and its parts.

use serde::{Deserialize, Serialize};

use super::{ChatRef, MessageBody};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", content = "c", rename_all = "snake_case")]
pub enum MessageAuthor {
    Me,
    Friend,
    Member(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Sending,
    Sent,
    Delivered,
    Failed,
    Received,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    pub id: Option<i64>,
    pub author: MessageAuthor,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub count: u32,
    pub mine: bool,
    pub who: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mention {
    pub name: String,
    pub member: i64,
    pub me: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i64,
    pub chat: ChatRef,
    pub author: MessageAuthor,
    pub body: MessageBody,
    /// Milliseconds since the epoch.
    pub at: i64,
    pub delivery: Delivery,
    pub unread: bool,
    pub reply: Option<Quote>,
    pub reactions: Vec<Reaction>,
    pub edited: bool,
    pub mentions: Vec<Mention>,
}
