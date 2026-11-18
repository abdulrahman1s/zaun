//! What the friend's side sends about the call (ADR-0035). A signal about a call that is not
//! the current one (an older call, another friend) is ignored.
use super::state::Phase;
use crate::chat::media::{CONNECT_TIMEOUT, CallKey, CallRequest, CallSignal, Negotiation};
use crate::model::CallLink;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn on_call_signal(&mut self, signal: CallSignal) -> Result<()> {
        match signal {
            CallSignal::Invited {
                friend,
                usable,
                key,
            } => self.on_call_invited(friend, usable, key),
            CallSignal::Offer {
                friend,
                sdp,
                candidates,
                key,
            } => return self.on_call_offer(friend, sdp, candidates, key),
            CallSignal::Answer {
                friend,
                sdp,
                candidates,
            } => {
                if let Some(m) = self.offering_media(friend) {
                    m.answer(sdp);
                    if !candidates.is_empty() {
                        m.candidates(candidates);
                    }
                }
            }
            CallSignal::Extra { friend, candidates } => {
                if let Some(m) = self.call_with(friend).and_then(|c| c.media.as_ref()) {
                    m.candidates(candidates);
                }
            }
            CallSignal::Declined(friend) => {
                if self.call_with(friend).is_some() {
                    tracing::info!(friend, "call: the friend declined");
                    self.call_close(None);
                }
            }
            CallSignal::Ended(friend) => {
                if self.call_with(friend).is_some() {
                    tracing::info!(friend, "call: the friend hung up");
                    self.call_close(None);
                }
            }
            CallSignal::Unusable(friend) => {
                if self.call_with(friend).is_some() {
                    self.call_close(Some(CallRequest::End));
                    bail!(
                        ChtCallFailed,
                        detail = "the friend's app sent a call description Zaun cannot read"
                    );
                }
            }
            CallSignal::Refused { friend, detail } => {
                if self.call_with(friend).is_some() {
                    self.call_close(None);
                    bail!(ChtCallFailed, detail = detail);
                }
            }
        }
        Ok(())
    }

    /// The friend answered the user's call with an offer: the media answers it.
    fn on_call_offer(
        &mut self,
        friend: i64,
        sdp: String,
        candidates: Vec<String>,
        key: Option<CallKey>,
    ) -> Result<()> {
        let Some(c) = self
            .call_with(friend)
            .filter(|c| c.phase == Phase::RingingOut)
        else {
            tracing::info!(friend, "call: an offer for no call of ours");
            return Ok(());
        };
        c.key = key;
        let number = c.number;
        let media = self
            .call_media(number, Negotiation::Answer(sdp))
            .inspect_err(|_| self.call_close(Some(CallRequest::End)))?;
        if !candidates.is_empty() {
            media.candidates(candidates);
        }
        if let Some(c) = &mut self.flow.chat.call {
            c.media = Some(media);
            c.phase = Phase::Connecting;
            c.link = CallLink::Connecting;
        }
        self.call_timer(number, Phase::Connecting, CONNECT_TIMEOUT);
        self.call_publish();
        Ok(())
    }

    /// The media of the call with `friend` that wrote an offer, waiting for its answer.
    fn offering_media(&mut self, friend: i64) -> Option<&dyn crate::chat::media::CallMedia> {
        self.call_with(friend)
            .filter(|c| c.offerer && c.phase == Phase::Connecting)
            .and_then(|c| c.media.as_deref())
    }
}
