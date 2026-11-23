//! A conversation as the chat list shows it.

use serde::{Deserialize, Serialize};

use super::{ChatMessage, ChatRef};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatPreview {
    pub chat: ChatRef,
    pub name: String,
    pub last: Option<ChatMessage>,
    pub unread: u32,
    pub muted: bool,
}
