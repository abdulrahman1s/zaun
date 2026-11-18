//! Decline the ringing call (ADR-0035): it ends at once for the caller too; both lines read
//! "Declined", and on this side it is not unread.
use super::state::Phase;
use crate::chat::media::CallRequest;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn call_decline(&mut self) {
        if self
            .flow
            .chat
            .call
            .as_ref()
            .is_some_and(|c| c.phase == Phase::RingingIn)
        {
            self.call_close(Some(CallRequest::Decline));
        }
    }
}
