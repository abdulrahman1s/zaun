//! The peer that moves its receive queue, built by hand on the in-memory server: what arrives on either queue is read raw and opened with its own keys and ratchet.
use super::rotation_support::{addr, create_queue};
use super::support::{Rig, WAIT};
use crate::agent::client::ConnId;
use crate::agent::delivery::open;
use crate::agent::envelope::AgentMsgEnvelope;
use crate::agent::message::{AMessage, AgentMessage, SndQAddr};
use crate::agent::records::{Batch, RcvQueue, SendKind};
use crate::agent::testing::{Node, pump};
use crate::agent::version::supported_client_range;
use crate::agent::{
    MsgIntegrity, QueueMode, SmpQueueAddress, SmpQueueUri, check_msg_integrity, msg_hash,
};
use crate::crypto::PublicKey;
use crate::encoding::Decode;
use crate::ratchet::RatchetError;

pub struct Peer {
    pub cb: ConnId,
    pub old: RcvQueue,
    pub new: RcvQueue,
    /// Agent messages in the order they first arrived; each was checked to follow the last in the chain.
    pub got: Vec<AMessage>,
    pub confs: Vec<PublicKey>,
    pub dups: usize,
    /// Off unless a test expects a gap: the integrity errors seen, instead of failing on the first.
    pub gaps: Option<Vec<MsgIntegrity>>,
}

impl Peer {
    /// Takes the peer's current queue off the server's pushes, and creates its new one.
    pub fn start(r: &Rig, a: &mut Node, b: &mut Node, cb: ConnId) -> Self {
        let old = b.agent.conn(cb).unwrap().rec.rcv.clone().unwrap();
        pump(&mut [a, b], WAIT, |_| {
            let i = r.smp.lock();
            let q = &i.queues.by_rcv[&old.rcv_id];
            (q.msgs.is_empty() && q.delivered.is_none()).then_some(())
        });
        r.smp
            .lock()
            .queues
            .by_rcv
            .get_mut(&old.rcv_id)
            .unwrap()
            .subscriber = None;
        // Nor does the peer's agent subscribe it again after a reconnect.
        let (mut rec, _) = b.agent.editable(cb).unwrap();
        rec.rcv = None;
        b.agent
            .save(&mut b.store, rec, None, Batch::default())
            .unwrap();
        Self {
            cb,
            old,
            new: create_queue(r),
            got: Vec::new(),
            confs: Vec::new(),
            dups: 0,
            gaps: None,
        }
    }

    pub fn uri(&self) -> SmpQueueUri {
        SmpQueueUri {
            client_v_range: supported_client_range(),
            address: SmpQueueAddress {
                server: self.new.server.clone(),
                sender_id: self.new.snd_id.clone(),
                dh_public_key: self.new.e2e_priv.public().unwrap(),
                queue_mode: Some(QueueMode::Messaging),
            },
        }
    }

    pub fn old_addr(&self) -> SndQAddr {
        addr(&self.old)
    }

    pub fn new_addr(&self) -> SndQAddr {
        addr(&self.new)
    }

    /// The peer's agent message to us, ratchet-encrypted as any other.
    pub fn say(&self, b: &mut Node, msg: AMessage) {
        b.agent
            .enqueue_message(&mut b.store, self.cb, msg, SendKind::Switch)
            .unwrap();
    }

    pub fn qadd(&self, b: &mut Node) {
        self.say(b, AMessage::Qadd(vec![(self.uri(), Some(self.old_addr()))]));
    }

    pub fn read_old(&mut self, r: &Rig, b: &mut Node) {
        let rq = self.old.clone();
        self.read(r, b, &rq);
    }

    pub fn read_new(&mut self, r: &Rig, b: &mut Node) {
        let rq = self.new.clone();
        self.read(r, b, &rq);
    }

    fn read(&mut self, r: &Rig, b: &mut Node, rq: &RcvQueue) {
        let bodies: Vec<Vec<u8>> = {
            let mut i = r.smp.lock();
            let q = i.queues.by_rcv.get_mut(&rq.rcv_id).unwrap();
            q.msgs.drain(..).filter_map(|(_, body)| body).collect()
        };
        for body in bodies {
            let mut rq = rq.clone();
            if rq.rcv_id == self.new.rcv_id {
                rq.peer_e2e.clone_from(&self.new.peer_e2e);
            }
            let o = open(&rq, &body).unwrap();
            match AgentMsgEnvelope::from_bytes(&o.envelope).unwrap() {
                AgentMsgEnvelope::Confirmation {
                    e2e_encryption: None,
                    enc_conn_info,
                    ..
                } => {
                    assert!(enc_conn_info.is_empty());
                    let key = o.sender_e2e.unwrap();
                    self.new.peer_e2e = Some(key.clone());
                    self.confs.push(key);
                }
                AgentMsgEnvelope::Message {
                    enc_agent_message, ..
                } => self.decrypt(b, &enc_agent_message),
                other => panic!("unexpected envelope {other:?}"),
            }
        }
    }

    fn decrypt(&mut self, b: &mut Node, enc: &[u8]) {
        let (mut rec, rc) = b.agent.editable(self.cb).unwrap();
        let mut rc = rc.unwrap();
        let plain = match rc.decrypt(enc) {
            Ok(p) => p,
            Err(RatchetError::DuplicateMessage | RatchetError::EarlierMessage(_)) => {
                self.dups += 1;
                return;
            }
            Err(e) => panic!("ratchet: {e}"),
        };
        let AgentMessage::Message { header, message } = AgentMessage::from_bytes(&plain).unwrap()
        else {
            panic!("not an agent message");
        };
        let c = &rec.chain;
        let integrity = check_msg_integrity(
            c.rcv_id,
            header.snd_msg_id,
            &c.rcv_hash,
            &header.prev_msg_hash,
        );
        match &mut self.gaps {
            Some(gaps) if integrity != MsgIntegrity::Ok => gaps.push(integrity),
            Some(_) | None => assert_eq!(integrity, MsgIntegrity::Ok, "{message:?} out of order"),
        }
        rec.chain.rcv_id = header.snd_msg_id;
        rec.chain.rcv_hash = msg_hash(&plain).to_vec();
        b.agent
            .save(&mut b.store, rec, Some(rc), Batch::default())
            .unwrap();
        self.got.push(message);
    }

    /// The chat bodies that arrived, in order.
    pub fn bodies(&self) -> Vec<&[u8]> {
        self.got
            .iter()
            .filter_map(|m| match m {
                AMessage::AMsg(b) => Some(b.as_slice()),
                _ => None,
            })
            .collect()
    }

    /// After the switch: the new queue becomes the peer agent's own, read normally from its next start.
    pub fn adopt(&self, b: &mut Node) {
        let (mut rec, _) = b.agent.editable(self.cb).unwrap();
        rec.rcv = Some(self.new.clone());
        b.agent
            .save(&mut b.store, rec, None, Batch::default())
            .unwrap();
    }
}
