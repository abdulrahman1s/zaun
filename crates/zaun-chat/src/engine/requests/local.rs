//! Local settings: mute, block a friend, block a group member. Nothing is sent (codec rules).

use zaun_chat_wire::model::ChatRef;

use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail, not_yet};
use crate::engine::machine::Engine;
use crate::engine::records::{FriendRecord, NS, keys};

impl<A: AgentPort> Engine<A> {
    pub(crate) fn friend(&self, id: i64) -> Result<FriendRecord> {
        self.state
            .friends
            .get(&id)
            .cloned()
            .ok_or_else(|| fail(Code::RequestFailed, "no such friend"))
    }

    pub(crate) fn set_muted(&mut self, chat: ChatRef, on: bool) -> Result<()> {
        let id = match chat {
            ChatRef::Direct(id) => id,
            ChatRef::Group(_) => return Err(not_yet(Code::RequestFailed)),
        };
        let mut f = self.friend(id)?;
        f.muted = on;
        self.save_friend(f)?;
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

    pub(crate) fn set_member_blocked(&mut self, group: i64, member: i64, on: bool) -> Result<()> {
        let key = keys::member_blocked(group, member);
        if on {
            self.store.put(NS, &key, &[1])?;
        } else {
            self.store.delete(NS, &key)?;
        }
        Ok(())
    }
}
