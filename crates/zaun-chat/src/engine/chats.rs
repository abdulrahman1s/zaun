//! The chat list: every friend, and a conversation for each one not blocked (codec `parse_chats`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatPreview, ChatRef, Friend};

use super::agent_port::AgentPort;
use super::machine::Engine;
use super::profile::picture_of;
use super::records::FriendRecord;

fn friend(f: &FriendRecord) -> Friend {
    Friend {
        id: f.id,
        name: f.name.clone(),
        ready: f.reachable(),
        about: f.about.clone(),
        since: Some(f.since),
        blocked: f.blocked,
        muted: f.muted,
        picture: f.image.as_deref().and_then(picture_of),
        ..Friend::default()
    }
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn chats(&self) -> Event {
        let friends = self.state.friends.values().map(friend).collect();
        // A blocked friend is reachable from "Blocked", not from the list.
        let chats = self
            .state
            .friends
            .values()
            .filter(|f| !f.blocked)
            .map(|f| ChatPreview {
                chat: ChatRef::Direct(f.id),
                name: f.name.clone(),
                last: self.state.last.get(&f.id).cloned(),
                unread: f.unread,
                muted: f.muted,
            })
            .collect();
        Event::Chats {
            friends,
            groups: Vec::new(),
            requests: Vec::new(),
            chats,
        }
    }
}
