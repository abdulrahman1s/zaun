//! SetProfile: the name and the line about the user, kept and sent as `x.info` to every friend.

use zaun_chat_wire::Event;
use zaun_chat_wire::model::ChatProfile;

use crate::chat::{ChatEvent, ChatMessage, encode_chat_message};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::profile::{MAX_PICTURE_URI, sent_profile};
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
        self.update_profile(user)
    }

    /// Commands.hs `updateProfile_`: checked and encoded before anything is kept, nothing sent
    /// when the profile as friends see it did not change.
    pub(super) fn update_profile(&mut self, user: UserRecord) -> Result<()> {
        let new = sent_profile(&user);
        if let Some(image) = &new.image
            && image.0.len() > MAX_PICTURE_URI
        {
            return Err(fail(
                Code::RequestFailed,
                format!("Profile image is too large {}", image.0.len()),
            ));
        }
        let changed = new != sent_profile(&self.state.user);
        let body = changed
            .then(|| {
                let msg = ChatMessage::new(ChatEvent::Info {
                    profile: new,
                    member_key: None,
                })?;
                encode_chat_message(&msg, true)
            })
            .transpose()?;
        self.save_user(user)?;
        self.emit(Event::Profile(self.state.user.to_wire()));
        if let Some(body) = body {
            self.send_to_friends(&body);
        }
        Ok(())
    }

    fn send_to_friends(&mut self, body: &[u8]) {
        let conns: Vec<_> = self
            .state
            .friends
            .values()
            .filter(|f| f.reachable())
            .map(|f| f.conn)
            .collect();
        for conn in conns {
            // One friend's relay failing is no reason to keep the profile from the rest.
            if let Err(e) = self.agent.send(&mut self.store, &conn, body.to_vec()) {
                self.log
                    .line(format_args!("profile not sent: {}", e.detail()));
            }
        }
    }
}
