//! A friend calls (ADR-0035): ring for `RING_TIMEOUT`. A call this build cannot take, or one
//! while the user is in another, is let go as missed.
use super::state::Phase;
use crate::chat::Request;
use crate::chat::media::{CallKey, CallRequest, RING_GRACE, RING_TIMEOUT};
use crate::service::prelude::*;

impl AppService {
    pub(super) fn on_call_invited(&mut self, friend: i64, usable: bool, key: Option<CallKey>) {
        let calls = self.flow.chat.snapshot.features.calls;
        // Both called each other at once: the engine keeps the friend's call, so this one
        // rings instead of the user's.
        let busy = self
            .flow
            .chat
            .call
            .as_ref()
            .is_some_and(|c| !(c.friend == friend && c.phase == Phase::RingingOut));
        if !usable || !calls || busy {
            tracing::info!(
                friend,
                usable,
                calls,
                busy,
                "call: an incoming call let go as missed"
            );
            self.chat_request(Request::Call(CallRequest::End(friend)));
            return;
        }
        self.flow.chat.call = None;
        let number = self.call_new(friend, Phase::RingingIn);
        if let Some(c) = &mut self.flow.chat.call {
            c.key = key;
        }
        self.call_timer(number, Phase::RingingIn, RING_TIMEOUT + RING_GRACE);
        self.call_publish();
    }
}
