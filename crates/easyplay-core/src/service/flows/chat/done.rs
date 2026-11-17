//! A Chat job finished, or the engine reported (ADR-0033).
use super::state::ChatDone;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn on_chat_done(&mut self, done: ChatDone) -> Result<()> {
        match done {
            ChatDone::Event { generation, event } => {
                if generation == self.flow.chat.generation {
                    self.on_chat_event(event)?;
                }
            }
            ChatDone::Restart { generation } => {
                let st = &self.flow.chat;
                if generation == st.generation && st.engine.is_none() && self.prefs.chat.enabled {
                    self.chat_launch()?;
                }
            }
            ChatDone::Saved(folder) => {
                let folder = folder?;
                self.emit(Event::Notice(Notice::ChatFileSaved { folder }));
            }
            ChatDone::Prepared { generation, result } => {
                if generation == self.flow.chat.generation {
                    self.chat_request(result?);
                }
            }
        }
        Ok(())
    }
}
