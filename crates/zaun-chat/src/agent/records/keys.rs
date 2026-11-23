//! Where the agent's records live in the store's `agent` namespace, and the writes that change them together.
use crate::encoding::{Decode, Encode};
use crate::ratchet::Ratchet;
use crate::store::{Op, Store};

use super::conn::ConnRecord;
use super::pending::PendingSend;
use super::received::{SentMsg, Unacked};
use super::request::StoredInvitation;
use crate::agent::client::{AgentMsgId, ConnId};
use crate::agent::error::Result;

pub const NS: &str = "agent";

/// Hashes of sent messages kept for receipts: older ones are dropped as new ones are sent.
pub const RECEIPT_WINDOW: i64 = 512;

fn key(tag: u8, conn: ConnId, sub: Option<i64>) -> Vec<u8> {
    let mut k = vec![tag, b'/'];
    k.extend_from_slice(&conn.0.to_be_bytes());
    if let Some(n) = sub {
        k.push(b'/');
        k.extend_from_slice(&n.to_be_bytes());
    }
    k
}

pub fn conn_key(c: ConnId) -> Vec<u8> {
    key(b'c', c, None)
}

pub fn ratchet_key(c: ConnId) -> Vec<u8> {
    key(b'r', c, None)
}

pub fn unacked_key(c: ConnId) -> Vec<u8> {
    key(b'u', c, None)
}

pub fn send_key(c: ConnId, id: AgentMsgId) -> Vec<u8> {
    key(b's', c, Some(id.0))
}

pub fn sent_key(c: ConnId, snd_msg_id: i64) -> Vec<u8> {
    key(b'h', c, Some(snd_msg_id))
}

/// A request received on address `c`; `n` is its number from the `agent.inv` counter.
pub fn invitation_key(c: ConnId, n: i64) -> Vec<u8> {
    key(b'i', c, Some(n))
}

/// The prefixes of everything stored for one connection.
pub fn conn_prefixes(c: ConnId) -> [Vec<u8>; 6] {
    [
        conn_key(c),
        ratchet_key(c),
        unacked_key(c),
        [key(b's', c, None), b"/".to_vec()].concat(),
        [key(b'h', c, None), b"/".to_vec()].concat(),
        [key(b'i', c, None), b"/".to_vec()].concat(),
    ]
}

/// The writes of one change, applied in one transaction.
#[derive(Default)]
pub struct Batch(pub Vec<Op>);

impl Batch {
    fn put(&mut self, key: Vec<u8>, v: &impl Encode) -> Result<&mut Self> {
        self.0.push(Op::Put {
            ns: NS,
            key,
            value: v.to_bytes()?,
        });
        Ok(self)
    }

    pub fn delete(&mut self, key: Vec<u8>) -> &mut Self {
        self.0.push(Op::Delete { ns: NS, key });
        self
    }

    pub fn conn(&mut self, rec: &ConnRecord) -> Result<&mut Self> {
        self.put(conn_key(rec.id), rec)
    }

    pub fn ratchet(&mut self, c: ConnId, rc: &Ratchet) -> Result<&mut Self> {
        self.put(ratchet_key(c), rc)
    }

    pub fn pending(&mut self, c: ConnId, p: &PendingSend) -> Result<&mut Self> {
        self.put(send_key(c, p.id), p)
    }

    pub fn unacked(&mut self, c: ConnId, u: &Unacked) -> Result<&mut Self> {
        self.put(unacked_key(c), u)
    }

    pub fn sent(&mut self, c: ConnId, snd_msg_id: i64, s: &SentMsg) -> Result<&mut Self> {
        self.put(sent_key(c, snd_msg_id), s)?;
        Ok(self.delete(sent_key(c, snd_msg_id - RECEIPT_WINDOW)))
    }

    pub fn invitation(&mut self, c: ConnId, n: i64, i: &StoredInvitation) -> Result<&mut Self> {
        self.put(invitation_key(c, n), i)
    }

    pub fn apply(self, store: &mut Store) -> Result<()> {
        store.apply(self.0)?;
        Ok(())
    }
}

pub fn load<T: Decode>(store: &mut Store, key: &[u8]) -> Result<Option<T>> {
    store
        .get(NS, key)?
        .map(|b| T::from_bytes(&b))
        .transpose()
        .map_err(Into::into)
}

/// Every connection id with a record.
pub fn conn_ids(store: &mut Store) -> Result<Vec<ConnId>> {
    let mut ids: Vec<ConnId> = store
        .keys_with_prefix(NS, b"c/")?
        .iter()
        .filter_map(|k| Some(ConnId(i64::from_be_bytes(k.get(2..10)?.try_into().ok()?))))
        .collect();
    ids.sort_unstable();
    Ok(ids)
}

/// The ids of a connection's pending sends, oldest first.
pub fn pending_ids(store: &mut Store, c: ConnId) -> Result<Vec<AgentMsgId>> {
    let prefix = [key(b's', c, None), b"/".to_vec()].concat();
    let mut ids: Vec<AgentMsgId> = store
        .keys_with_prefix(NS, &prefix)?
        .iter()
        .filter_map(|k| {
            let tail = k.get(prefix.len()..)?;
            Some(AgentMsgId(i64::from_be_bytes(tail.try_into().ok()?)))
        })
        .collect();
    ids.sort_unstable();
    Ok(ids)
}
