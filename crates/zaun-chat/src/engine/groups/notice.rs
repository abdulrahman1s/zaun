//! A notice line in a group's conversation (codec `a_groups_story_is_told_in_notices_…`): stored, shown, and the list refreshed.

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatNotice, ChatRef, MessageAuthor, MessageBody};

use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::ItemRecord;

impl<A: AgentPort> Engine<A> {
    /// `member`: who the notice is about when it is a member's doing; `None` for the user's own
    /// or the group's.
    pub(crate) fn group_notice(
        &mut self,
        group: i64,
        member: Option<i64>,
        notice: ChatNotice,
    ) -> Result<()> {
        let author = match member.and_then(|m| self.state.members.get(&(group, m))) {
            Some(m) => MessageAuthor::Member(m.profile.display_name.clone()),
            None => MessageAuthor::Me,
        };
        let mut item = ItemRecord::new(author, MessageBody::Notice(notice), self.clock.wall_ms());
        item.member = member;
        let m = self.add_item(ChatRef::Group(group), &item)?;
        self.emit(Event::Messages(vec![m]));
        self.state.chats_dirty = true;
        Ok(())
    }
}
