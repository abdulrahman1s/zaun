//! The user's stream requests: the same calls as video without the apps' encryption (ADR-0036); a stream has no line.

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{StreamRequest, StreamSignal};

use super::line::Over;
use super::state::Kind;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    /// A refused invite, offer or answer is reported to the stream and forgets it; a refused
    /// status or end is only logged. An end with no stream sends nothing.
    pub(crate) fn stream(&mut self, request: StreamRequest) -> Result<()> {
        let (friend, loud, r) = match request {
            StreamRequest::Invite(f) => (f, true, self.place(f, Kind::Stream)),
            StreamRequest::Offer { friend, sdp } => {
                (friend, true, self.offer(friend, Kind::Stream, &sdp))
            }
            StreamRequest::Answer { friend, sdp } => {
                (friend, true, self.answer(friend, Kind::Stream, &sdp))
            }
            StreamRequest::Connected(f) => (f, false, self.connected(f, Kind::Stream)),
            StreamRequest::End(f) => (f, false, self.hang_up(f, Some(Kind::Stream), Over::Ended)),
        };
        if let Err(e) = r {
            self.log
                .line(format_args!("a stream request failed: {}", e.detail));
            if loud {
                self.forget_stream(friend);
                self.emit(Event::Stream(StreamSignal::Refused {
                    friend,
                    detail: e.detail,
                }));
            }
        }
        Ok(())
    }

    fn forget_stream(&mut self, friend: i64) {
        if self
            .state
            .calls
            .calls
            .get(&friend)
            .is_some_and(|c| c.kind == Kind::Stream)
        {
            self.state.calls.calls.remove(&friend);
        }
    }
}
