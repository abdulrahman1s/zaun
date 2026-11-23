//! Local settings: mute, block a friend, block a group member. Nothing is sent (codec rules).

use zaun_chat_wire::model::ChatRef;

use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::FriendRecord;

impl<A: AgentPort> Engine<A> {
    pub(crate) fn friend(&self, id: i64) -> Result<FriendRecord> {
        self.state
            .friends
            .get(&id)
            .cloned()
            .ok_or_else(|| fail(Code::RequestFailed, "no such friend"))
    }

    /// A muted group still marks mentions of the user (codec: `enableNtfs: mentions`): the
    /// page decides what to notify, the engine keeps counting.
    pub(crate) fn set_muted(&mut self, chat: ChatRef, on: bool) -> Result<()> {
        match chat {
            ChatRef::Direct(id) => {
                let mut f = self.friend(id)?;
                f.muted = on;
                self.save_friend(f)?;
            }
            ChatRef::Group(id) => {
                let mut g = self.group(id)?;
                g.muted = on;
                self.save_group(g)?;
            }
        }
        self.state.chats_dirty = true;
        Ok(())
    }

    /// They are not told; what they send while blocked is deleted on arrival.
    pub(crate) fn set_blocked(&mut self, id: i64, on: bool) -> Result<()> {
        let mut f = self.friend(id)?;
        f.blocked = on;
        self.save_friend(f)?;
        self.state.chats_dirty = true;
        Ok(())
    }

    /// Local (`showMessages: false`): what they send from now on is kept but collapsed and
    /// never counted as unread. The members list follows, as the codec read it again.
    pub(crate) fn set_member_blocked(&mut self, group: i64, member: i64, on: bool) -> Result<()> {
        let mut m = self.member(group, member)?;
        m.blocked = on;
        self.save_member(m)?;
        if let Err(e) = self.members(group) {
            self.log
                .line(format_args!("members not listed: {}", e.detail));
        }
        Ok(())
    }
}
