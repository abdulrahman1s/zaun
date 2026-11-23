//! A member's changed profile: kept, and told in the conversation when the name changed (Subscriber.hs `processMemberProfileUpdate`).

use zaun_chat_wire::model::ChatNotice;

use crate::chat::profile::Profile;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    /// `tell`: the profile came in a message (`x.info`), not in the handshake, which shows nothing.
    pub(crate) fn update_member_profile(
        &mut self,
        group: i64,
        member: i64,
        profile: &Profile,
        tell: bool,
    ) -> Result<()> {
        let mut m = self.member(group, member)?;
        if m.profile == *profile {
            return Ok(());
        }
        let from = std::mem::replace(&mut m.profile, profile.clone()).display_name;
        self.save_member(m)?;
        self.state.chats_dirty = true;
        if tell && from != profile.display_name {
            let to = profile.display_name.clone();
            let notice = ChatNotice::NameChanged { from, to };
            self.group_notice(group, Some(member), notice)?;
        }
        Ok(())
    }
}
