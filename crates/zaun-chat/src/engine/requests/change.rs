//! Edit: the user changes a message the others know, in a direct chat or a group, and what
//! Edit and Delete share (Commands.hs `APIUpdateChatItem`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatRef, MessageAuthor, MessageBody};

use crate::chat::container::MsgMentions;
use crate::chat::content::MsgContent;
use crate::chat::group::GroupMemberRole;
use crate::chat::ids::SharedMsgId;
use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::ItemRecord;

use super::invite::PQ;

const DAY_MS: i64 = 24 * 3600 * 1000;
/// Messages.hs `deletable'`: a day to edit; deleting gets six hours more (`assertDeletable`).
const EDIT_MS: i64 = DAY_MS;
pub(super) const DELETE_MS: i64 = DAY_MS + 6 * 3600 * 1000;

/// Messages.hs `ciMsgContent`: what a message carries, not a notice, call line or deletion.
pub(crate) fn has_content(body: &MessageBody) -> bool {
    match body {
        MessageBody::Text(_)
        | MessageBody::Voice { .. }
        | MessageBody::Gif(_)
        | MessageBody::Image(_)
        | MessageBody::Video(_)
        | MessageBody::File(_) => true,
        MessageBody::Notice(_)
        | MessageBody::Call(_)
        | MessageBody::Stream(_)
        | MessageBody::Deleted => false,
    }
}

impl<A: AgentPort> Engine<A> {
    /// One of the user's own messages, still within `window` of being sent: its shared id.
    pub(super) fn own_item(
        &mut self,
        chat: ChatRef,
        item: i64,
        window: i64,
    ) -> Result<(ItemRecord, Vec<u8>)> {
        let it = self
            .item(chat, item)?
            .ok_or_else(|| fail(Code::RequestFailed, "no such item"))?;
        let fresh = self.clock.wall_ms().saturating_sub(it.at) < window;
        let own = it.author == MessageAuthor::Me && has_content(&it.body);
        match it.shared_id.clone() {
            Some(s) if own && fresh => Ok((it, s)),
            Some(_) | None => Err(fail(Code::RequestFailed, "item can't change")),
        }
    }

    pub(super) fn send_event(&mut self, friend: i64, event: ChatEvent) -> Result<()> {
        let f = self.friend(friend)?;
        if !f.reachable() {
            return Err(fail(Code::RequestFailed, "not connected"));
        }
        let body = encode_chat_message(&ChatMessage::new(event)?, PQ)?;
        self.agent.send(&mut self.store, &f.conn, body)?;
        Ok(())
    }

    /// To the friend, or to every member of the group reachable now.
    pub(super) fn send_to_chat(&mut self, chat: ChatRef, event: ChatEvent) -> Result<()> {
        match chat {
            ChatRef::Direct(friend) => self.send_event(friend, event),
            ChatRef::Group(group) => {
                self.send_to_group(group, &ChatMessage::new(event)?, None)?;
                Ok(())
            }
        }
    }

    pub(crate) fn save_changed(&mut self, chat: ChatRef, item: i64, it: &ItemRecord) -> Result<()> {
        let m = self.change_item(chat, item, it)?;
        let m = self.shown_last(chat, it, m);
        self.emit(Event::Updated(vec![m]));
        Ok(())
    }

    /// In a group, the user must still be in it with `min` (Commands.hs `assertUserGroupRole`).
    pub(super) fn may_change(&self, chat: ChatRef, min: &GroupMemberRole) -> Result<()> {
        match chat {
            ChatRef::Direct(_) => Ok(()),
            ChatRef::Group(g) => Self::assert_my_role(&self.group(g)?, min),
        }
    }

    pub(crate) fn edit(&mut self, chat: ChatRef, item: i64, text: &str) -> Result<()> {
        self.may_change(chat, &GroupMemberRole::Author)?;
        let (mut it, shared) = self.own_item(chat, item, EDIT_MS)?;
        if it.body == MessageBody::Text(text.to_owned()) {
            return Ok(());
        }
        let MessageBody::Text(_) = it.body else {
            return Err(fail(Code::RequestFailed, "only text can be edited"));
        };
        let in_group = matches!(chat, ChatRef::Group(_));
        self.send_to_chat(
            chat,
            ChatEvent::MsgUpdate {
                msg_id: SharedMsgId(shared),
                content: MsgContent::Text {
                    text: text.to_owned(),
                },
                mentions: MsgMentions::new(),
                ttl: None,
                live: None,
                scope: None,
                as_group: in_group.then_some(false),
            },
        )?;
        it.body = MessageBody::Text(text.to_owned());
        it.edited = true;
        // The edit carries no mentions, so the item has none after it (`updateGroupCIMentions`).
        it.mentions.clear();
        self.save_changed(chat, item, &it)
    }
}
