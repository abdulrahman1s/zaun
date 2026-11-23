//! MarkRead: the conversation is read up to its newest item (local; the codec reports nothing).

use zaun_chat_wire::model::ChatRef;

use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn mark_read(&mut self, chat: ChatRef) -> Result<()> {
        let (read_up_to, unread) = match chat {
            ChatRef::Direct(id) => {
                let f = self.friend(id)?;
                (f.read_up_to, f.unread)
            }
            ChatRef::Group(id) => {
                let g = self.group(id)?;
                (g.read_up_to, g.unread)
            }
        };
        let newest = self.state.last.get(&chat).map_or(read_up_to, |m| m.id);
        if unread == 0 && read_up_to >= newest {
            return Ok(());
        }
        match chat {
            ChatRef::Direct(id) => {
                let mut f = self.friend(id)?;
                (f.read_up_to, f.unread) = (newest, 0);
                self.save_friend(f)?;
            }
            ChatRef::Group(id) => {
                let mut g = self.group(id)?;
                (g.read_up_to, g.unread) = (newest, 0);
                self.save_group(g)?;
            }
        }
        if let Some(last) = self.state.last.get_mut(&chat) {
            last.unread = false;
        }
        Ok(())
    }
}
