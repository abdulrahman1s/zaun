//! History: a page of a conversation, oldest first, before an item or from the newest.

use zaun_chat_wire::Event;
use zaun_chat_wire::model::ChatRef;

use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;

/// The most items one page returns, whatever was asked.
pub const MAX_PAGE: u32 = 500;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn history(
        &mut self,
        chat: ChatRef,
        before: Option<i64>,
        count: u32,
        tag: u64,
    ) -> Result<()> {
        let messages = match chat {
            ChatRef::Direct(id) => {
                self.friend(id)?;
                self.items_page(id, before, count.min(MAX_PAGE))?
            }
            // No groups yet: nothing to read.
            ChatRef::Group(_) => Vec::new(),
        };
        self.emit(Event::History {
            tag,
            chat,
            messages,
        });
        Ok(())
    }
}
