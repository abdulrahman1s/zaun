//! What a call's media reports (ADR-0035): its description goes to the friend, the call
//! starts counting when sound gets through, and a media failure ends the call.
use crate::chat::Request;
use crate::chat::media::{CallRequest, MediaEvent};
use crate::model::{CallLink, CallQuality};
use crate::service::prelude::*;

impl AppService {
    pub(super) fn on_call_media(&mut self, number: u64, event: MediaEvent) -> Result<()> {
        let now = self.deps.clock.now();
        let Some(c) = self.flow.chat.call.as_mut().filter(|c| c.number == number) else {
            return Ok(());
        };
        let friend = c.friend;
        match event {
            MediaEvent::Description(sdp) => {
                let r = if c.offerer {
                    CallRequest::Offer { friend, sdp }
                } else {
                    CallRequest::Answer { friend, sdp }
                };
                self.chat_request(Request::Call(r));
                return Ok(());
            }
            MediaEvent::Connected => {
                let first = c.since.is_none();
                c.phase = super::state::Phase::Live;
                c.link = CallLink::Connected;
                c.since.get_or_insert(now);
                if first {
                    tracing::info!(friend, "call: connected");
                    self.chat_request(Request::Call(CallRequest::Connected(friend)));
                }
            }
            MediaEvent::Interrupted => c.link = CallLink::Interrupted,
            MediaEvent::Levels { me, friend: theirs } => {
                // A tenth's change is not worth a new snapshot.
                if c.levels.me / 10 == me / 10 && c.levels.friend / 10 == theirs / 10 {
                    return Ok(());
                }
                c.levels.me = me;
                c.levels.friend = theirs;
            }
            MediaEvent::Quality { rtt_ms, loss } => {
                let q = CallQuality { rtt_ms, loss };
                // A new snapshot only for a change the bars or their tooltip would show.
                if c.quality
                    .is_some_and(|o| o.loss == loss && o.rtt_ms.abs_diff(rtt_ms) < 10)
                {
                    return Ok(());
                }
                c.quality = Some(q);
            }
            MediaEvent::Failed { code, detail } => {
                tracing::warn!(
                    friend,
                    code = code.as_str(),
                    detail,
                    "call: the media failed"
                );
                self.call_close(Some(CallRequest::End));
                return Err(Error::new(code).with("detail", detail));
            }
        }
        self.call_publish();
        Ok(())
    }
}
