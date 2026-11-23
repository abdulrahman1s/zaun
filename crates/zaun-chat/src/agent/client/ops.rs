//! Commands sent and not yet answered, by tag: what to continue when the tagged reply arrives.
use std::collections::HashMap;

use super::ids::{AgentMsgId, ConnId};
use crate::smp::SmpServer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpKind {
    /// NEW of an invitation queue or of the joiner's reply queue.
    NewQueue,
    Lkey,
    /// LGET of a contact short link: public, nothing is secured.
    Lget,
    /// SEND of our request to an address queue.
    Request,
    /// SKEY on the peer's queue, by either side.
    Skey,
    Sub,
    Ack,
    Send(AgentMsgId),
    Del,
}

#[derive(Debug, Clone)]
pub(crate) struct Op {
    pub conn: ConnId,
    pub kind: OpKind,
    pub server: SmpServer,
}

#[derive(Default)]
pub(crate) struct Ops {
    next: u64,
    /// Eviction: an entry leaves with its reply, which smp sends for every tagged request
    /// (an error when the session ends); `forget_server` drops those of a closed connection.
    map: HashMap<u64, Op>,
}

impl Ops {
    pub(crate) fn next_tag(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    pub(crate) fn insert(&mut self, tag: u64, op: Op) {
        self.map.insert(tag, op);
    }

    pub(crate) fn take(&mut self, tag: u64) -> Option<Op> {
        self.map.remove(&tag)
    }

    pub(crate) fn servers(&self) -> impl Iterator<Item = SmpServer> + '_ {
        self.map.values().map(|op| op.server.clone())
    }

    pub(crate) fn forget_server(&mut self, keep: impl Fn(&SmpServer) -> bool) {
        self.map.retain(|_, op| keep(&op.server));
    }
}
