//! Chat items of a conversation, direct or group: added, read back by number or shared id, changed.

use zaun_chat_wire::model::{ChatMessage, ChatRef};

use super::agent_port::AgentPort;
use super::machine::Engine;
use super::records::{ItemRecord, decode, encode, keys};
use crate::store::StoreError;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn read_mark(&self, chat: ChatRef) -> i64 {
        match chat {
            ChatRef::Direct(f) => self.state.friends.get(&f).map_or(0, |f| f.read_up_to),
            ChatRef::Group(g) => self.state.groups.get(&g).map_or(0, |g| g.read_up_to),
        }
    }

    /// Stores a new item (and its shared id), and returns it as the page sees it.
    pub(crate) fn add_item(
        &mut self,
        chat: ChatRef,
        item: &ItemRecord,
    ) -> Result<ChatMessage, StoreError> {
        let id = self.store.insert_item(&keys::chat(chat), &encode(item)?)?;
        if let Some(shared) = &item.shared_id {
            self.put_index(&keys::shared(chat, item.member, shared), id)?;
        }
        let m = item.to_wire(id, chat, self.read_mark(chat));
        self.state.last.insert(chat, m.clone());
        Ok(m)
    }

    pub(crate) fn item(
        &mut self,
        chat: ChatRef,
        id: i64,
    ) -> Result<Option<ItemRecord>, StoreError> {
        let page = self.store.page(&keys::chat(chat), id.checked_add(1), 1)?;
        match page.into_iter().next() {
            Some((found, bytes)) if found == id => decode(&bytes).map(Some),
            Some(_) | None => Ok(None),
        }
    }

    /// `member`: in a group, the author's member number (`None` = the user's own item).
    pub(crate) fn item_by_shared(
        &mut self,
        chat: ChatRef,
        member: Option<i64>,
        shared: &[u8],
    ) -> Result<Option<(i64, ItemRecord)>, StoreError> {
        let Some(id) = self.get_index(&keys::shared(chat, member, shared))? else {
            return Ok(None);
        };
        Ok(self.item(chat, id)?.map(|i| (id, i)))
    }

    /// Writes a changed item back, and returns it as the page sees it.
    pub(crate) fn change_item(
        &mut self,
        chat: ChatRef,
        id: i64,
        item: &ItemRecord,
    ) -> Result<ChatMessage, StoreError> {
        self.store
            .update_item(&keys::chat(chat), id, &encode(item)?)?;
        let m = item.to_wire(id, chat, self.read_mark(chat));
        if self.state.last.get(&chat).is_some_and(|l| l.id == id) {
            self.state.last.insert(chat, m.clone());
        }
        Ok(m)
    }

    /// The newest `count` items before `before`, oldest first.
    pub(crate) fn items_page(
        &mut self,
        chat: ChatRef,
        before: Option<i64>,
        count: u32,
    ) -> Result<Vec<ChatMessage>, StoreError> {
        let mark = self.read_mark(chat);
        self.store
            .page(&keys::chat(chat), before, count)?
            .into_iter()
            .map(|(id, b)| {
                let item: ItemRecord = decode(&b)?;
                Ok(item.to_wire(id, chat, mark))
            })
            .collect()
    }
}
