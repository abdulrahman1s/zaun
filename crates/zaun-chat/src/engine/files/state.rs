//! A file item's record in the store and its state on the page.

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatRef, FileState};

use super::body::{state_of, with_state};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail, not_yet};
use crate::engine::machine::Engine;
use crate::engine::records::file::FileRecord;
use crate::engine::records::{ItemRecord, NS, decode, keys};

impl<A: AgentPort> Engine<A> {
    pub(super) fn friend_of_chat(chat: ChatRef) -> Result<i64> {
        match chat {
            ChatRef::Direct(id) => Ok(id),
            ChatRef::Group(_) => Err(not_yet(Code::RequestFailed)),
        }
    }

    pub(super) fn file_record(&mut self, friend: i64, item: i64) -> Result<Option<FileRecord>> {
        let Some(bytes) = self.store.get(NS, &keys::file(friend, item))? else {
            return Ok(None);
        };
        Ok(Some(decode(&bytes)?))
    }

    pub(super) fn save_file(&mut self, friend: i64, item: i64, rec: &FileRecord) -> Result<()> {
        self.put(&keys::file(friend, item), rec)?;
        Ok(())
    }

    /// An item and its file record, or the failure of a request about a message with no file.
    pub(super) fn file_of(&mut self, friend: i64, item: i64) -> Result<(ItemRecord, FileRecord)> {
        let found = match (self.item(friend, item)?, self.file_record(friend, item)?) {
            (Some(it), Some(rec)) => Some((it, rec)),
            (Some(_) | None, Some(_) | None) => None,
        };
        found.ok_or_else(|| fail(Code::RequestFailed, "no file known for that message"))
    }

    /// What the page shows for the file now.
    pub(super) fn file_state(it: &ItemRecord, rec: &FileRecord) -> Option<FileState> {
        state_of(&it.body, rec.way)
    }

    /// Shows `state` on the item and tells the page. A deleted item or one that is no file stays as it is.
    pub(super) fn show_file(
        &mut self,
        friend: i64,
        item: i64,
        rec: &FileRecord,
        state: FileState,
    ) -> Result<()> {
        let Some(mut it) = self.item(friend, item)? else {
            return Ok(());
        };
        let Some(body) = with_state(&it.body, rec.kind, state, rec.local.clone()) else {
            return Ok(());
        };
        if body == it.body {
            return Ok(());
        }
        it.body = body;
        let m = self.change_item(friend, item, &it)?;
        self.emit(Event::Updated(vec![m]));
        Ok(())
    }
}
