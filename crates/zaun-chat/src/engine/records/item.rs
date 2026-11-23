//! One chat item (Messages.hs `ChatItem`) in the store's items table; its row id is the item number.

use serde::{Deserialize, Serialize};
use zaun_chat_wire::model::{
    ChatMessage, ChatRef, Delivery, Mention, MessageAuthor, MessageBody, Quote, Reaction,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemRecord {
    pub author: MessageAuthor,
    pub body: MessageBody,
    /// Milliseconds since the epoch.
    pub at: i64,
    pub delivery: Delivery,
    pub reply: Option<Quote>,
    pub reactions: Vec<Reaction>,
    pub edited: bool,
    pub mentions: Vec<Mention>,
    /// The `SharedMsgId` bytes both sides know the message by; notices have none.
    pub shared_id: Option<Vec<u8>>,
}

impl ItemRecord {
    #[must_use]
    pub fn new(author: MessageAuthor, body: MessageBody, at: i64) -> Self {
        let delivery = match author {
            MessageAuthor::Me => Delivery::Sending,
            MessageAuthor::Friend | MessageAuthor::Member(_) => Delivery::Received,
        };
        Self {
            author,
            body,
            at,
            delivery,
            reply: None,
            reactions: Vec::new(),
            edited: false,
            mentions: Vec::new(),
            shared_id: None,
        }
    }

    /// The item as the page sees it; `read_up_to` is the conversation's read mark.
    #[must_use]
    pub fn to_wire(&self, id: i64, chat: ChatRef, read_up_to: i64) -> ChatMessage {
        let notice = matches!(self.body, MessageBody::Notice(_));
        ChatMessage {
            id,
            chat,
            author: self.author.clone(),
            body: self.body.clone(),
            at: self.at,
            delivery: self.delivery,
            unread: self.author != MessageAuthor::Me && !notice && id > read_up_to,
            reply: self.reply.clone(),
            reactions: self.reactions.clone(),
            edited: self.edited,
            mentions: self.mentions.clone(),
        }
    }
}
