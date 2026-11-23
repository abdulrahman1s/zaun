//! A call not connected in time is let go (Chat.hs `callInvitationTTL`); core's own ring timeout normally ends it first.

use zaun_chat_wire::Event;
use zaun_chat_wire::model::{CallSignal, StreamSignal};

use super::line::Over;
use super::state::Kind;
use crate::engine::agent_port::AgentPort;
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    /// The earliest wall ms at which a call expires.
    #[must_use]
    pub(crate) fn next_call_wake(&self) -> Option<i64> {
        self.state.calls.next_deadline()
    }

    /// Lets go of every call due by `now`: the friend is told when the user placed it, the
    /// line reads missed (or ended, when it was answered), and the page's flow is told it is over.
    pub(crate) fn expire_calls(&mut self, now: i64) {
        for friend in self.state.calls.expired(now) {
            let Some(call) = self.state.calls.calls.remove(&friend) else {
                continue;
            };
            if call.outgoing {
                self.send_end(friend, &call.id);
            }
            self.close_call(friend, &call, Over::Ended);
            self.emit(match call.kind {
                Kind::Voice => Event::Call(CallSignal::Ended(friend)),
                Kind::Stream => Event::Stream(StreamSignal::Ended(friend)),
            });
        }
    }
}
