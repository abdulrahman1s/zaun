//! Answer the ringing call (ADR-0035): the media writes the offer, which goes to the friend.
use super::state::Phase;
use crate::chat::media::{CONNECT_TIMEOUT, CallRequest, Negotiation};
use crate::model::CallLink;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn call_answer(&mut self) -> Result<()> {
        let Some(c) = self
            .flow
            .chat
            .call
            .as_ref()
            .filter(|c| c.phase == Phase::RingingIn)
        else {
            return Ok(());
        };
        let (number, friend) = (c.number, c.friend);
        let media = self
            .call_media(number, Negotiation::Offer)
            .inspect_err(|_| self.call_close(Some(CallRequest::End)))?;
        if let Some(c) = &mut self.flow.chat.call {
            c.media = Some(media);
            c.offerer = true;
            c.phase = Phase::Connecting;
            c.link = CallLink::Connecting;
        }
        tracing::info!(friend, "call: answered");
        self.call_timer(number, Phase::Connecting, CONNECT_TIMEOUT);
        self.call_publish();
        // The offer is sent when the media has written it (`MediaEvent::Description`).
        Ok(())
    }
}
