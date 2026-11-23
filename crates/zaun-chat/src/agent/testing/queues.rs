//! The in-memory server's queues: one message delivered at a time per subscriber, a quota, sender keys set once.
use std::collections::{HashMap, VecDeque};

use crate::crypto::{Alg, CbNonce, KeyPair, PublicKey, cb_encrypt, dh, random_vec};
use crate::encoding::{Encode, SysTime};
use crate::smp::{ErrorType, QueueLinkData, Response};

pub struct Queue {
    pub rcv_id: Vec<u8>,
    pub snd_id: Vec<u8>,
    pub rcv_auth: PublicKey,
    pub rcv_dh: PublicKey,
    pub server_dh: KeyPair,
    pub sender_key: Option<PublicKey>,
    pub link: Option<(Vec<u8>, QueueLinkData)>,
    /// An address's queue: senders use it unsigned and it is never secured.
    pub contact: bool,
    /// `None` is the queue-drained notice after QUOTA.
    pub msgs: VecDeque<(Vec<u8>, Option<Vec<u8>>)>,
    pub delivered: Option<Vec<u8>>,
    pub subscriber: Option<u64>,
    pub quota_hit: bool,
}

#[derive(Default)]
pub struct Queues {
    pub by_rcv: HashMap<Vec<u8>, Queue>,
    pub by_snd: HashMap<Vec<u8>, Vec<u8>>,
    pub by_link: HashMap<Vec<u8>, Vec<u8>>,
    /// Messages a queue holds before SEND gets QUOTA.
    pub quota: usize,
}

/// A push for one session: (session, entity, response).
pub type Push = (u64, Vec<u8>, Response);

impl Queues {
    pub fn secure(&mut self, snd_id: &[u8], key: PublicKey) -> Response {
        let Some(q) = self.by_snd.get(snd_id).and_then(|r| self.by_rcv.get_mut(r)) else {
            return Response::Err(ErrorType::Auth);
        };
        match &q.sender_key {
            Some(k) if *k != key => Response::Err(ErrorType::Auth),
            _ => {
                q.sender_key = Some(key);
                Response::Ok
            }
        }
    }

    pub fn send(&mut self, snd_id: &[u8], body: Vec<u8>, pushes: &mut Vec<Push>) -> Response {
        let quota = self.quota;
        let Some(q) = self.by_snd.get(snd_id).and_then(|r| self.by_rcv.get_mut(r)) else {
            return Response::Err(ErrorType::Auth);
        };
        if q.msgs.len() >= quota {
            q.quota_hit = true;
            return Response::Err(ErrorType::Quota);
        }
        q.msgs.push_back((random_vec(24).unwrap(), Some(body)));
        deliver(q, pushes);
        Response::Ok
    }

    pub fn ack(&mut self, rcv_id: &[u8], msg_id: &[u8], pushes: &mut Vec<Push>) -> Response {
        let Some(q) = self.by_rcv.get_mut(rcv_id) else {
            return Response::Err(ErrorType::Auth);
        };
        if q.delivered.as_deref() != Some(msg_id) {
            return Response::Err(ErrorType::NoMsg);
        }
        q.delivered = None;
        q.msgs.pop_front();
        if q.msgs.is_empty() && q.quota_hit {
            q.quota_hit = false;
            q.msgs.push_back((random_vec(24).unwrap(), None));
        }
        deliver(q, pushes);
        Response::Ok
    }
}

/// Pushes the head message to the subscriber unless one is waiting for its ACK.
pub fn deliver(q: &mut Queue, pushes: &mut Vec<Push>) {
    let (Some(session), None, Some((msg_id, body))) = (q.subscriber, &q.delivered, q.msgs.front())
    else {
        return;
    };
    let ts = SysTime(1_700_000_000).to_bytes().unwrap();
    let plain = match body {
        Some(b) => [ts.as_slice(), b"T ", b].concat(),
        None => [b"QUOTA ".as_slice(), &ts].concat(),
    };
    let secret = dh(&q.rcv_dh, &q.server_dh.private).unwrap();
    let nonce = CbNonce::from_slice(msg_id).unwrap();
    let body = cb_encrypt(&secret, &nonce, &plain, plain.len() + 2).unwrap();
    q.delivered = Some(msg_id.clone());
    let msg = Response::Msg {
        msg_id: msg_id.clone(),
        body,
    };
    pushes.push((session, q.rcv_id.clone(), msg));
}

pub fn new_queue(rcv_auth: PublicKey, rcv_dh: PublicKey, snd_id: Vec<u8>) -> Queue {
    Queue {
        rcv_id: random_vec(24).unwrap(),
        snd_id,
        rcv_auth,
        rcv_dh,
        server_dh: KeyPair::generate(Alg::X25519).unwrap(),
        sender_key: None,
        link: None,
        contact: false,
        msgs: VecDeque::new(),
        delivered: None,
        subscriber: None,
        quota_hit: false,
    }
}
