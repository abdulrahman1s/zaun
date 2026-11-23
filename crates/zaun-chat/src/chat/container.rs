//! The `x.msg.new` params and what hangs off a message (Protocol.hs `MsgContainer`, `MsgRef`, `QuotedMsg`, `MsgScope`).
use super::content::MsgContent;
use super::file::FileInvitation;
use super::ids::{MemberId, SharedMsgId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MsgScope {
    #[serde(rename_all = "camelCase")]
    Member { member_id: MemberId },
}

/// `sent_at` is the JSON time text as received; never reformat it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MsgRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub msg_id: Option<SharedMsgId>,
    pub sent_at: String,
    pub sent: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<MemberId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotedMsg {
    pub msg_ref: MsgRef,
    pub content: MsgContent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MsgMention {
    pub member_id: MemberId,
}

/// Mentions by the display name as written in the text; the map is sorted like the reference's.
pub type MsgMentions = BTreeMap<String, MsgMention>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardLink {
    pub display_name: String,
    pub group_link: String,
    pub public_group_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_id: Option<MemberId>,
    pub msg_id: SharedMsgId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MsgContainer {
    pub content: MsgContent,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mentions: MsgMentions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<FileInvitation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<MsgScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_group: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<QuotedMsg>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<MsgRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forward_link: Option<ForwardLink>,
}

impl MsgContainer {
    /// `mcSimple`.
    pub fn simple(content: MsgContent) -> Self {
        Self {
            content,
            mentions: MsgMentions::new(),
            file: None,
            ttl: None,
            live: None,
            scope: None,
            as_group: None,
            quote: None,
            parent: None,
            forward: None,
            forward_link: None,
        }
    }

    pub fn text(text: &str) -> Self {
        Self::simple(MsgContent::Text {
            text: text.to_owned(),
        })
    }

    /// `mcQuote`.
    pub fn quote(quote: QuotedMsg, content: MsgContent) -> Self {
        Self {
            quote: Some(quote),
            ..Self::simple(content)
        }
    }

    /// `mcForward`.
    pub fn forward(link: Option<ForwardLink>, content: MsgContent) -> Self {
        Self {
            forward: Some(true),
            forward_link: link,
            ..Self::simple(content)
        }
    }
}
