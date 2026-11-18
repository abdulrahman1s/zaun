//! Call a friend (ADR-0035): ring them; their answer brings an offer, which the media answers.
use super::state::Phase;
use crate::chat::Request;
use crate::chat::media::{CallRequest, RING_TIMEOUT};
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn call_place(&mut self, chat: ChatRef) -> Result<()> {
        if !self.flow.chat.snapshot.features.calls || self.flow.chat.call.is_some() {
            tracing::debug!(?chat, "call: not placed (calls off, or a call already)");
            return Ok(());
        }
        let ChatRef::Direct(friend) = chat else {
            bail!(ChtCallFailed, detail = "group voice is not carried yet");
        };
        let snap = &self.flow.chat.snapshot;
        let ready = snap.friend(friend).is_some_and(|f| f.ready && !f.blocked);
        ensure!(
            ready,
            ChtCallFailed,
            detail = "not a friend who can be called"
        );
        let number = self.call_new(friend, Phase::RingingOut);
        self.chat_request(Request::Call(CallRequest::Invite(friend)));
        self.call_timer(number, Phase::RingingOut, RING_TIMEOUT);
        self.call_publish();
        Ok(())
    }
}
