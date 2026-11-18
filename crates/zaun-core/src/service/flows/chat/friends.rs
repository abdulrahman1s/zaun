//! Friends and conversations: mute, block, security code (ADR-0033). The snapshot changes at
//! once; the engine is asked after.
use crate::chat::Request;
use crate::service::prelude::*;

impl AppService {
    pub(super) fn chat_set_muted(&mut self, chat: ChatRef, on: bool) {
        let snap = &mut self.flow.chat.snapshot;
        if let Some(p) = snap.preview_mut(chat) {
            p.muted = on;
        }
        if chat.is_direct() {
            if let Some(f) = snap.friend_mut(chat.id()) {
                f.muted = on;
            }
        } else if let Some(g) = snap.group_mut(chat.id()) {
            g.muted = on;
        }
        self.chat_publish();
        self.chat_request(Request::SetMuted { chat, on });
    }

    pub(super) fn chat_set_blocked(&mut self, friend: i64, on: bool) {
        let snap = &mut self.flow.chat.snapshot;
        if let Some(f) = snap.friend_mut(friend) {
            f.blocked = on;
        }
        if on {
            snap.chats.retain(|p| p.chat != ChatRef::Direct(friend));
        }
        self.chat_publish();
        self.chat_request(Request::SetBlocked { friend, on });
    }

    pub(super) fn chat_show_code(&mut self, friend: i64) {
        if self.flow.chat.snapshot.code.take().is_some() {
            self.chat_publish();
        }
        self.chat_request(Request::ShowCode(friend));
    }
}
