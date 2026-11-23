//! Chat items of a direct conversation: added, read back by number or shared id, changed.

use zaun_chat_wire::model::{ChatMessage, ChatRef};

use super::agent_port::AgentPort;
use super::machine::Engine;
use super::records::{ItemRecord, decode, encode, keys};
use crate::store::StoreError;

impl<A: AgentPort> Engine<A> {
    fn read_mark(&self, friend: i64) -> i64 {
        self.state.friends.get(&friend).map_or(0, |f| f.read_up_to)
    }

    /// Stores a new item (and its shared id), and returns it as the page sees it.
    pub(crate) fn add_item(
        &mut self,
        friend: i64,
        item: &ItemRecord,
    ) -> Result<ChatMessage, StoreError> {
        let id = self
            .store
            .insert_item(&keys::direct_chat(friend), &encode(item)?)?;
        if let Some(shared) = &item.shared_id {
            self.put_index(&keys::shared(friend, shared), id)?;
        }
        let m = item.to_wire(id, ChatRef::Direct(friend), self.read_mark(friend));
        self.state.last.insert(friend, m.clone());
        Ok(m)
    }

    pub(crate) fn item(&mut self, friend: i64, id: i64) -> Result<Option<ItemRecord>, StoreError> {
        let page = self
            .store
            .page(&keys::direct_chat(friend), id.checked_add(1), 1)?;
        match page.into_iter().next() {
            Some((found, bytes)) if found == id => decode(&bytes).map(Some),
            Some(_) | None => Ok(None),
        }
    }

    pub(crate) fn item_by_shared(
        &mut self,
        friend: i64,
        shared: &[u8],
    ) -> Result<Option<(i64, ItemRecord)>, StoreError> {
        let Some(id) = self.get_index(&keys::shared(friend, shared))? else {
            return Ok(None);
        };
        Ok(self.item(friend, id)?.map(|i| (id, i)))
    }

    /// Writes a changed item back, and returns it as the page sees it.
    pub(crate) fn change_item(
        &mut self,
        friend: i64,
        id: i64,
        item: &ItemRecord,
    ) -> Result<ChatMessage, StoreError> {
        self.store
            .update_item(&keys::direct_chat(friend), id, &encode(item)?)?;
        let m = item.to_wire(id, ChatRef::Direct(friend), self.read_mark(friend));
        if self.state.last.get(&friend).is_some_and(|l| l.id == id) {
            self.state.last.insert(friend, m.clone());
        }
        Ok(m)
    }

    /// The newest `count` items before `before`, oldest first.
    pub(crate) fn items_page(
        &mut self,
        friend: i64,
        before: Option<i64>,
        count: u32,
    ) -> Result<Vec<ChatMessage>, StoreError> {
        let mark = self.read_mark(friend);
        self.store
            .page(&keys::direct_chat(friend), before, count)?
            .into_iter()
            .map(|(id, b)| {
                let item: ItemRecord = decode(&b)?;
                Ok(item.to_wire(id, ChatRef::Direct(friend), mark))
            })
            .collect()
    }
}
