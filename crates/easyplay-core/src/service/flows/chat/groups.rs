//! Groups: new group, and blocking a member (ADR-0033).
use crate::chat::Request;
use crate::service::prelude::*;

impl AppService {
    pub(super) fn chat_new_group(&mut self, name: &str) {
        let name = name.trim();
        if !name.is_empty() {
            self.chat_request(Request::NewGroup(name.to_owned()));
        }
    }

    pub(super) fn chat_set_member_blocked(&mut self, group: i64, member: i64, on: bool) {
        let found = (self.flow.chat.snapshot.group_mut(group))
            .and_then(|g| g.member_list.iter_mut().find(|m| m.id == member));
        if let Some(m) = found {
            m.blocked = on;
            self.chat_publish();
        }
        self.chat_request(Request::SetMemberBlocked { group, member, on });
    }
}
