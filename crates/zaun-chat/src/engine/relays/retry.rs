//! A request that reached no relay is made once more after a check (the old codec's `after_check`), and one waiting on a relay a check just disabled is made again.

use zaun_chat_wire::RequestKind;

use crate::engine::agent_port::{AgentFailure, AgentPort, ConnId};
use crate::engine::error::{EngineError, Result};
use crate::engine::machine::Engine;
use crate::engine::records::ConnKind;

/// What is made again: an invitation or an address (a connection is retried by the agent itself).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Retry {
    Invite,
    Address,
}

impl Retry {
    const fn slot(self) -> usize {
        match self {
            Self::Invite => 0,
            Self::Address => 1,
        }
    }

    const fn request(self) -> RequestKind {
        match self {
            Self::Invite => RequestKind::CreateInvite,
            Self::Address => RequestKind::Address,
        }
    }
}

impl<A: AgentPort> Engine<A> {
    /// `conn`, the invitation or address being made, failed. A network failure the first time
    /// waits for a relay check and is made again; whether it was taken (the caller then ends
    /// without reporting). The request stays marked as running, so a second tap is ignored.
    pub(crate) fn retry_after_check(
        &mut self,
        kind: Retry,
        conn: &ConnId,
        err: &AgentFailure,
    ) -> bool {
        if !matches!(err, AgentFailure::Network(_)) {
            return false;
        }
        if self.relays.retried[kind.slot()] == Some(*conn) {
            self.relays.retried[kind.slot()] = None;
            return false;
        }
        self.log.line(format_args!(
            "a relay did not answer ({}); checking the relays, then once more",
            err.detail()
        ));
        self.drop_attempt(kind, conn);
        if !self.relays.after.contains(&kind) {
            self.relays.after.push(kind);
        }
        if !self.start_check() && self.relays.check.is_none() {
            self.flush_retries();
        }
        true
    }

    pub(super) fn flush_retries(&mut self) {
        for kind in std::mem::take(&mut self.relays.after) {
            self.make_again(kind);
        }
    }

    /// Whatever the user is waiting for on a relay that was just disabled starts over on another.
    pub(super) fn restart_pending_queues(&mut self) {
        let (waiting_invite, waiting_address) = (
            self.relays.after.contains(&Retry::Invite),
            self.relays.after.contains(&Retry::Address),
        );
        let invite = self.state.inviting.filter(|c| {
            !waiting_invite
                && matches!(
                    self.state.conns.get(c).map(|r| &r.kind),
                    Some(ConnKind::Invitation { full: None, .. })
                )
        });
        if let Some(conn) = invite {
            self.drop_attempt(Retry::Invite, &conn);
            self.make_again(Retry::Invite);
        }
        let address = self.state.address.creating.filter(|_| !waiting_address);
        if let Some(conn) = address {
            self.drop_attempt(Retry::Address, &conn);
            self.make_again(Retry::Address);
        }
    }

    fn drop_attempt(&mut self, kind: Retry, conn: &ConnId) {
        let gone = match kind {
            Retry::Invite => self.forget_conn(conn),
            Retry::Address => self
                .agent
                .delete(&mut self.store, conn)
                .map_err(EngineError::from),
        };
        if let Err(e) = gone {
            self.log.line(format_args!(
                "a failed attempt was not removed: {}",
                e.detail
            ));
        }
    }

    fn make_again(&mut self, kind: Retry) {
        let made: Result<()> = match kind {
            Retry::Invite => {
                self.state.inviting = None;
                self.create_invite()
            }
            Retry::Address => {
                self.state.address.creating = None;
                self.create_address()
            }
        };
        let now = match kind {
            Retry::Invite => self.state.inviting,
            Retry::Address => self.state.address.creating,
        };
        self.relays.retried[kind.slot()] = now;
        if let Err(e) = made {
            let _ = self.fail_event(kind.request(), e); // `fail_event` only emits
        }
    }
}
