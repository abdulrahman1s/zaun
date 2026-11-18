//! A call's timer ran out (ADR-0035): unanswered ringing becomes a missed call; an answered
//! call whose sound never got through failed to connect.
use super::state::Phase;
use crate::chat::media::CallRequest;
use crate::service::prelude::*;

impl AppService {
    pub(super) fn on_call_timeout(&mut self, number: u64, phase: Phase) -> Result<()> {
        let current = self
            .flow
            .chat
            .call
            .as_ref()
            .is_some_and(|c| c.number == number && c.phase == phase);
        if !current {
            return Ok(());
        }
        tracing::info!(?phase, "call: timed out");
        self.call_close(Some(CallRequest::End));
        ensure!(
            phase != Phase::Connecting,
            ChtCallFailed,
            detail = "no sound got through in time"
        );
        Ok(())
    }
}
