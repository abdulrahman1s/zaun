//! Replies: what a quote carries out (`QuotedMsg`) and what the page shows of one (codec `parse_item`).

use zaun_chat_wire::model::{MessageAuthor, MessageBody, Quote};

use super::records::ItemRecord;
use super::time::iso;
use crate::chat::container::{MsgRef, QuotedMsg};
use crate::chat::content::MsgContent;
use crate::chat::ids::SharedMsgId;

/// The text of a body, for quoting; what has none quotes as empty.
#[must_use]
pub fn body_text(body: &MessageBody) -> &str {
    match body {
        MessageBody::Text(t) => t,
        MessageBody::Image(m) | MessageBody::Video(m) | MessageBody::File(m) => &m.caption,
        MessageBody::Notice(_)
        | MessageBody::Voice { .. }
        | MessageBody::Gif(_)
        | MessageBody::Call(_)
        | MessageBody::Stream(_)
        | MessageBody::Deleted => "",
    }
}

/// The page shows a quote's first line only.
#[must_use]
pub fn shown_quote(id: Option<i64>, author: MessageAuthor, text: &str) -> Quote {
    Quote {
        id,
        author,
        text: text.lines().next().unwrap_or("").to_owned(),
    }
}

/// The quote of `item` (number `id`) as sent; `None` for a notice, which has no shared id.
#[must_use]
pub fn outgoing(id: i64, item: &ItemRecord) -> Option<(QuotedMsg, Quote)> {
    let shared = item.shared_id.clone()?;
    let text = body_text(&item.body);
    let quoted = QuotedMsg {
        msg_ref: MsgRef {
            msg_id: Some(SharedMsgId(shared)),
            sent_at: iso(item.at),
            sent: item.author == MessageAuthor::Me,
            member_id: None,
        },
        content: MsgContent::Text {
            text: text.to_owned(),
        },
    };
    Some((quoted, shown_quote(Some(id), item.author.clone(), text)))
}
