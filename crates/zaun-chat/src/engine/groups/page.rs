//! How a group's items show: a blocked member's are never unread (codec: they collapse
//! without counting), on arrival, when changed and in History.

use zaun_chat_wire::model::{ChatMessage, ChatRef};

use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{ItemRecord, decode, keys};

impl<A: AgentPort> Engine<A> {
    pub(crate) fn member_blocked(&self, group: i64, member: i64) -> bool {
        self.state
            .members
            .get(&(group, member))
            .is_some_and(|m| m.blocked)
    }

    /// `m` as the page sees it: unread only if its author is not blocked.
    pub(crate) fn shown(&self, chat: ChatRef, it: &ItemRecord, mut m: ChatMessage) -> ChatMessage {
        if let (ChatRef::Group(g), Some(member)) = (chat, it.member)
            && self.member_blocked(g, member)
        {
            m.unread = false;
        }
        m
    }

    /// An item just stored or changed as the page sees it, and the chat list's copy of it
    /// too (`add_item` and `change_item` put it there as `to_wire` made it).
    pub(crate) fn shown_last(
        &mut self,
        chat: ChatRef,
        it: &ItemRecord,
        m: ChatMessage,
    ) -> ChatMessage {
        let m = self.shown(chat, it, m);
        if self.state.last.get(&chat).is_some_and(|l| l.id == m.id) {
            self.state.last.insert(chat, m.clone());
        }
        m
    }

    /// The newest `count` items of the group before `before`, oldest first.
    pub(crate) fn group_page(
        &mut self,
        group: i64,
        before: Option<i64>,
        count: u32,
    ) -> Result<Vec<ChatMessage>> {
        let chat = ChatRef::Group(group);
        let mark = self.read_mark(chat);
        let rows = self.store.page(&keys::chat(chat), before, count)?;
        let mut out = Vec::with_capacity(rows.len());
        for (id, bytes) in rows {
            let item: ItemRecord = decode(&bytes)?;
            out.push(self.shown(chat, &item, item.to_wire(id, chat, mark)));
        }
        Ok(out)
    }
}
