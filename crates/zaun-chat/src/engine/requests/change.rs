//! Edit and Delete: the user changes a message both sides know (Commands.hs
//! `APIUpdateChatItem`, `APIDeleteChatItem` broadcast).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatRef, MessageAuthor, MessageBody};

use crate::chat::container::MsgMentions;
use crate::chat::content::MsgContent;
use crate::chat::ids::SharedMsgId;
use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail, not_yet};
use crate::engine::machine::Engine;
use crate::engine::records::friend::Feature;
use crate::engine::records::{ItemRecord, NS, keys};

use super::invite::PQ;

const DAY_MS: i64 = 24 * 3600 * 1000;
/// Messages.hs `deletable'`: a day to edit; deleting gets six hours more (`assertDeletable`).
const EDIT_MS: i64 = DAY_MS;
const DELETE_MS: i64 = DAY_MS + 6 * 3600 * 1000;

/// Messages.hs `ciMsgContent`: what a message carries, not a notice, call line or deletion.
pub(super) fn has_content(body: &MessageBody) -> bool {
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
    pub(super) fn direct_friend(chat: ChatRef) -> Result<i64> {
        match chat {
            ChatRef::Direct(id) => Ok(id),
            ChatRef::Group(_) => Err(not_yet(Code::RequestFailed)),
        }
    }

    /// One of the user's own messages, still within `window` of being sent: its shared id.
    fn own_item(&mut self, friend: i64, item: i64, window: i64) -> Result<(ItemRecord, Vec<u8>)> {
        let it = self
            .item(friend, item)?
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

    pub(super) fn save_changed(&mut self, friend: i64, item: i64, it: &ItemRecord) -> Result<()> {
        let m = self.change_item(friend, item, it)?;
        self.emit(Event::Updated(vec![m]));
        Ok(())
    }

    pub(crate) fn edit(&mut self, chat: ChatRef, item: i64, text: &str) -> Result<()> {
        let friend = Self::direct_friend(chat)?;
        let (mut it, shared) = self.own_item(friend, item, EDIT_MS)?;
        if it.body == MessageBody::Text(text.to_owned()) {
            return Ok(());
        }
        let MessageBody::Text(_) = it.body else {
            return Err(fail(Code::RequestFailed, "only text can be edited"));
        };
        self.send_event(
            friend,
            ChatEvent::MsgUpdate {
                msg_id: SharedMsgId(shared),
                content: MsgContent::Text {
                    text: text.to_owned(),
                },
                mentions: MsgMentions::new(),
                ttl: None,
                live: None,
                scope: None,
                as_group: None,
            },
        )?;
        it.body = MessageBody::Text(text.to_owned());
        it.edited = true;
        self.save_changed(friend, item, &it)
    }

    /// Commands.hs `APIDeleteChatItem` broadcast: the item goes for good when the preferences
    /// allow full deletion for the user, else it stays, marked deleted.
    pub(crate) fn delete(&mut self, chat: ChatRef, item: i64) -> Result<()> {
        let friend = Self::direct_friend(chat)?;
        let (it, shared) = self.own_item(friend, item, DELETE_MS)?;
        self.send_event(
            friend,
            ChatEvent::MsgDel {
                msg_id: SharedMsgId(shared),
                member_id: None,
                scope: None,
                only_history: false,
            },
        )?;
        let full = self.friend(friend)?.enabled(Feature::FullDelete).for_user;
        self.finish_delete(friend, item, it, full)
    }

    /// Marks the item deleted, or removes it from the store; the page is told it is deleted either way.
    pub(crate) fn finish_delete(
        &mut self,
        friend: i64,
        item: i64,
        mut it: ItemRecord,
        full: bool,
    ) -> Result<()> {
        it.body = MessageBody::Deleted;
        it.reactions.clear();
        if !full {
            return self.save_changed(friend, item, &it);
        }
        self.store.delete_item(&keys::direct_chat(friend), item)?;
        if let Some(shared) = &it.shared_id {
            self.store.delete(NS, &keys::shared(friend, shared))?;
        }
        let read = self.state.friends.get(&friend).map_or(0, |f| f.read_up_to);
        let m = it.to_wire(item, ChatRef::Direct(friend), read);
        if self.state.last.get(&friend).is_some_and(|l| l.id == item) {
            self.state.last.insert(friend, m.clone());
        }
        self.emit(Event::Updated(vec![m]));
        Ok(())
    }
}
