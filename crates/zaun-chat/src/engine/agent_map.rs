//! The agent's ids, errors, servers and events as the engine's port has them (Agent/Protocol.hs, AGPL-3.0-only).

use super::agent_port::{AgentEvent, AgentFailure, AgentMsgId, ConnId};
use super::log::Log;
use crate::agent::AgentError;
use crate::agent::client::{self, AgentEvent as Native};
use crate::smp::SmpServer;

pub(super) const fn conn_id(c: client::ConnId) -> ConnId {
    ConnId(c.0)
}

pub(super) const fn native_conn(c: &ConnId) -> client::ConnId {
    client::ConnId(c.0)
}

pub(super) const fn native_msg(m: AgentMsgId) -> client::AgentMsgId {
    client::AgentMsgId(m)
}

/// The host the engine tracks as up or down; never the password.
pub(super) fn host(server: &SmpServer) -> String {
    server.hosts.first().cloned().unwrap_or_default()
}

pub(super) fn failure(e: &AgentError) -> AgentFailure {
    let detail = e.to_string();
    match e {
        AgentError::Smp(s) if client::is_temporary(s) => AgentFailure::Network(detail),
        AgentError::NotAvailable => AgentFailure::Network(detail),
        AgentError::Parse(_)
        | AgentError::MissingParam(_)
        | AgentError::Link(_)
        | AgentError::Decode(_) => AgentFailure::BadLink(detail),
        AgentError::Smp(_)
        | AgentError::Encode(_)
        | AgentError::Crypto(_)
        | AgentError::Store(_)
        | AgentError::Ratchet(_)
        | AgentError::Version
        | AgentError::Prohibited(_)
        | AgentError::ConnNotFound
        | AgentError::MsgNotFound
        | AgentError::Expired
        | AgentError::Unsupported(_) => AgentFailure::Other(detail),
    }
}

/// `None` for what the engine does not act on (logged where it says something).
pub(super) fn event(e: Native, log: &Log) -> Option<AgentEvent> {
    Some(match e {
        Native::Invitation { conn, full, short } => AgentEvent::Invitation {
            conn: conn_id(conn),
            full,
            short,
        },
        Native::Conf { conn, conn_info } => AgentEvent::Conf {
            conn: conn_id(conn),
            conn_info,
        },
        Native::Info { conn, conn_info } => AgentEvent::Info {
            conn: conn_id(conn),
            conn_info,
        },
        Native::Con { conn } => AgentEvent::Con {
            conn: conn_id(conn),
        },
        // The port carries neither the integrity verdict nor the broker time yet.
        Native::Msg {
            conn, msg_id, body, ..
        } => AgentEvent::Msg {
            conn: conn_id(conn),
            msg_id: msg_id.0,
            body,
        },
        Native::Sent { conn, msg_id } => AgentEvent::Sent {
            conn: conn_id(conn),
            msg_id: msg_id.0,
        },
        Native::Merr { conn, msg_id, err } => AgentEvent::Merr {
            conn: conn_id(conn),
            msg_id: msg_id.0,
            err: failure(&err),
        },
        Native::Rcvd { conn, msg_id, .. } => AgentEvent::Rcvd {
            conn: conn_id(conn),
            msg_id: msg_id.0,
        },
        Native::Up { server } => AgentEvent::Up {
            server: host(&server),
        },
        Native::Down { server } => AgentEvent::Down {
            server: host(&server),
        },
        Native::Err { conn, err } => AgentEvent::Err {
            conn: conn_id(conn),
            err: failure(&err),
        },
        Native::Mwarn { conn, msg_id, err } => {
            log.line(format_args!(
                "connection {} message {}: still sending: {err}",
                conn.0, msg_id.0
            ));
            return None;
        }
        Native::Qcont { conn } => {
            log.line(format_args!(
                "connection {}: the peer's queue has room",
                conn.0
            ));
            return None;
        }
        Native::End { conn } => {
            log.line(format_args!(
                "connection {}: another client took its queue",
                conn.0
            ));
            return None;
        }
        // TODO(phase 3): contact addresses; the engine does not answer requests yet.
        Native::Req { .. } => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smp::SmpError;

    #[test]
    fn a_dropped_connection_is_a_network_failure_and_a_refusal_is_not() {
        let io = AgentError::Smp(SmpError::Io("reset".into()));
        assert!(matches!(failure(&io), AgentFailure::Network(_)));
        let refused = AgentError::Smp(SmpError::LargeMsg);
        assert!(matches!(failure(&refused), AgentFailure::Other(_)));
    }

    #[test]
    fn a_bad_link_is_a_bad_link_failure() {
        assert!(matches!(
            failure(&AgentError::Parse("x")),
            AgentFailure::BadLink(_)
        ));
    }

    #[test]
    fn the_host_never_carries_the_password() {
        let s: SmpServer =
            "smp://hpq7_4gGJiilmz5Rf-CswuU5kZGkm_zOIooSw6yALRg=:secret@relay.test:5223"
                .parse()
                .unwrap();
        assert_eq!(host(&s), "relay.test");
    }
}
