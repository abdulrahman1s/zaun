//! The chat list: every friend and group, and a conversation for each friend not blocked and each group (codec `parse_chats`).

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{ChatPreview, ChatRef, Friend, FriendRequest};

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
        verified: f.verified.is_some(),
        picture: f.image.as_deref().and_then(picture_of),
        ..Friend::default()
    }
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn chats(&self) -> Event {
        let friends = self.state.friends.values().map(friend).collect();
        // A blocked friend is reachable from "Blocked", not from the list.
        let direct = self
            .state
            .friends
            .values()
            .filter(|f| !f.blocked)
            .map(|f| ChatPreview {
                chat: ChatRef::Direct(f.id),
                name: f.name.clone(),
                last: self.state.last.get(&ChatRef::Direct(f.id)).cloned(),
                unread: f.unread,
                muted: f.muted,
            });
        let in_groups = self.state.groups.values().map(|g| ChatPreview {
            chat: ChatRef::Group(g.id),
            name: g.profile.display_name.clone(),
            last: self.state.last.get(&ChatRef::Group(g.id)).cloned(),
            unread: g.unread,
            muted: g.muted,
        });
        let chats = direct.chain(in_groups).collect();
        let groups = self
            .state
            .groups
            .values()
            .map(|g| self.group_wire(g))
            .collect();
        let requests = self
            .state
            .address
            .requests
            .values()
            .map(|r| FriendRequest {
                id: r.id,
                name: r.name.clone(),
            })
            .collect();
        Event::Chats {
            friends,
            groups,
            requests,
            chats,
        }
    }
}
