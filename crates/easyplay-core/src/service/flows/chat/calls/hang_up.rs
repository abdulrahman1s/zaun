//! Leave the call, or stop calling (ADR-0035). The engine turns the call's line into "Call
//! ended, N min" (or a missed call when it never connected).
use super::state::Phase;
use crate::chat::media::CallRequest;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn call_hang_up(&mut self) {
        if self.flow.chat.call.is_some() {
            self.call_close(Some(CallRequest::End));
        }
    }

    /// The friend was blocked or removed: a call with them ends (a ringing one silently).
    pub(crate) fn call_drop_friend(&mut self, friend: i64) {
        let Some(c) = self.call_with(friend) else {
            return;
        };
        let request = if c.phase == Phase::RingingIn {
            CallRequest::Reject
        } else {
            CallRequest::End
        };
        self.call_close(Some(request));
    }
}
