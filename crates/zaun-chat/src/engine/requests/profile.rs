//! SetProfile: the name and the line about the user, kept and sent as `x.info` to every friend.

use zaun_chat_wire::Event;
use zaun_chat_wire::model::ChatProfile;

use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::profile::sent_profile;
use crate::engine::records::UserRecord;

impl<A: AgentPort> Engine<A> {
    /// The picture is not part of it (`SetPicture`), so the one set stays.
    pub(crate) fn set_profile(&mut self, p: &ChatProfile) -> Result<()> {
        let user = UserRecord {
            name: p.name.trim().to_owned(),
            about: p.about.trim().to_owned(),
            paper: p.paper,
            status: p.status,
            picture: self.state.user.picture.clone(),
        };
        self.save_user(user)?;
        self.emit(Event::Profile(self.state.user.to_wire()));
        self.send_profile_to_friends()
    }

    fn send_profile_to_friends(&mut self) -> Result<()> {
        let msg = ChatMessage::new(ChatEvent::Info {
            profile: sent_profile(&self.state.user),
            member_key: None,
        })?;
        let body = encode_chat_message(&msg, true)?;
        let conns: Vec<_> = self
            .state
            .friends
            .values()
            .filter(|f| f.reachable())
            .map(|f| f.conn)
            .collect();
        for conn in conns {
            // One friend's relay failing is no reason to keep the profile from the rest.
            if let Err(e) = self.agent.send(&mut self.store, &conn, body.clone()) {
                self.log
                    .line(format_args!("profile not sent: {}", e.detail()));
            }
        }
        Ok(())
    }
}
