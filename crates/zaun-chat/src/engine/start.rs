//! The start: records back, the user made on a first start, Features, Ready, subscriptions, the list.

use zaun_chat_wire::{Event, Hello, REACTION_EMOJI, features_with};

use super::agent_port::AgentPort;
use super::load::load;
use super::machine::Engine;
use super::records::UserRecord;
use crate::store::StoreError;

impl<A: AgentPort> Engine<A> {
    /// Requests are taken only after this, so one made before `Ready` waits.
    pub fn start(&mut self, hello: &Hello) -> Result<(), StoreError> {
        let (state, user) = load(&mut self.store)?;
        self.state = state;
        self.load_address()?;
        match user {
            Some(u) => self.state.user = u,
            None => self.save_user(UserRecord::named(&hello.name))?,
        }
        self.emit(Event::Features {
            features: features_with(&hello.relays),
            reactions: Some(REACTION_EMOJI.iter().map(|e| (*e).to_owned()).collect()),
        });
        self.emit(Event::Ready(self.state.user.to_wire()));
        self.configure_files(hello)?;
        self.configure_relays(hello);
        if let Err(e) = self.agent.subscribe_all(&mut self.store) {
            self.log
                .line(format_args!("subscribing failed: {}", e.detail()));
        }
        self.pump();
        // Out now, not lazily: what a waiting request answers must come after the list.
        self.state.chats_dirty = false;
        let chats = self.chats();
        self.emit(chats);
        self.announce_address();
        self.start_check();
        Ok(())
    }
}
