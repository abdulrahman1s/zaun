//! The user as joiner, on the connection with its host: CON makes the user a member, told as "you joined" and the host's joining (Subscriber.hs:886-925, `maybeCreateGroupDescrLocal`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatNotice, ChatRef, MessageAuthor, MessageBody};

use super::role::history_on;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{ItemRecord, MemberRecord, MemberStatus};

impl<A: AgentPort> Engine<A> {
    pub(crate) fn on_host_con(&mut self, group: i64, mut h: MemberRecord) -> Result<()> {
        let mut g = self.group(group)?;
        h.status = MemberStatus::Connected;
        let (host, name) = (h.id, h.profile.display_name.clone());
        self.save_member(h)?;
        let waits = matches!(
            g.status,
            MemberStatus::PendingApproval | MemberStatus::PendingReview
        );
        if !waits {
            g.status = MemberStatus::Connected;
        }
        let welcome = g
            .profile
            .description
            .clone()
            .filter(|_| !waits && !history_on(&g));
        self.save_group(g)?;
        // `CIRcvGroupE2EEInfo`, then `memberConnectedChatItem` (codec `history_group_left`).
        self.group_notice(group, Some(host), ChatNotice::YouJoined)?;
        self.group_notice(group, Some(host), ChatNotice::MemberJoined(name.clone()))?;
        let Some(text) = welcome else {
            return Ok(());
        };
        // Without history the welcome message is shown from the group's description.
        let mut item = ItemRecord::new(
            MessageAuthor::Member(name),
            MessageBody::Text(text),
            self.clock.wall_ms(),
        );
        item.member = Some(host);
        let m = self.add_item(ChatRef::Group(group), &item)?;
        self.emit(Event::Messages(vec![m]));
        Ok(())
    }
}
