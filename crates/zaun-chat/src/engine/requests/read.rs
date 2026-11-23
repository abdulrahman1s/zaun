//! MarkRead: the conversation is read up to its newest item (local; the codec reports nothing).

use zaun_chat_wire::model::ChatRef;

use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, not_yet};
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn mark_read(&mut self, chat: ChatRef) -> Result<()> {
        let id = match chat {
            ChatRef::Direct(id) => id,
            ChatRef::Group(_) => return Err(not_yet(Code::RequestFailed)),
        };
        let mut f = self.friend(id)?;
        let newest = self.state.last.get(&id).map_or(f.read_up_to, |m| m.id);
        if f.unread == 0 && f.read_up_to >= newest {
            return Ok(());
        }
        f.read_up_to = newest;
        f.unread = 0;
        self.save_friend(f)?;
        if let Some(last) = self.state.last.get_mut(&id) {
            last.unread = false;
        }
        Ok(())
    }
}
