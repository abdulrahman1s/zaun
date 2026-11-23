//! The re-sync rules one message at a time: who sends first, repeated keys, answers to keys we did not send, the reply we send.
use super::resync::{ad, state};
use super::support::{connect, rig};
use crate::agent::client::{AgentEvent, ConnId};
use crate::agent::delivery::{ClientMessage, ClientMsgEnvelope};
use crate::agent::error::AgentError;
use crate::agent::msg_hash;
use crate::agent::records::{self, Batch, PendingSend, pending_ids};
use crate::agent::resync::{SyncState, ratchet_info};
use crate::agent::testing::Node;
use crate::agent::version::CURRENT_AGENT;
use crate::agent::{AgentMsgEnvelope, E2eParams};
use crate::crypto::{cb_decrypt, dh};
use crate::encoding::{Decode, Encode, Version};
use crate::ratchet::{
    CURRENT_E2E_VERSION, PqSupport, PrivParams, Ratchet, RatchetVersions, RcvParams,
    e2e_enc_agent_msg_length, new_resync_params, ratchet_key_hash,
};

fn hash_of(p: &RcvParams) -> Vec<u8> {
    ratchet_key_hash(&p.params().k1, &p.params().k2).to_vec()
}

fn new_key() -> (PrivParams, RcvParams) {
    new_resync_params(CURRENT_E2E_VERSION, PqSupport(false)).unwrap()
}

fn wire(p: &RcvParams) -> E2eParams {
    E2eParams::from_bytes(&p.params().to_bytes().unwrap()).unwrap()
}

fn give_key(
    n: &mut Node,
    c: ConnId,
    p: &RcvParams,
    answered: Option<Vec<u8>>,
) -> Result<(), AgentError> {
    let info = ratchet_info(answered).unwrap();
    n.agent
        .on_ratchet_key(&mut n.store, c, Version(8), wire(p), info)
        .map(|_| ())
}

fn pending(n: &mut Node, c: ConnId) -> usize {
    pending_ids(&mut n.store, c).unwrap().len()
}

fn set_state(n: &mut Node, c: ConnId, s: SyncState) {
    let (mut rec, _) = n.agent.editable(c).unwrap();
    rec.sync_mut().state = s;
    n.agent
        .save(&mut n.store, rec, None, Batch::default())
        .unwrap();
}

#[test]
fn the_side_with_the_smaller_key_hash_receives_and_the_other_sends_first() {
    let (x, xp) = new_key();
    let (y, yp) = new_key();
    let v = RatchetVersions::new(CURRENT_E2E_VERSION);
    let rx = Ratchet::resync(v, &x, &yp, PqSupport(false)).unwrap();
    let ry = Ratchet::resync(v, &y, &xp, PqSupport(false)).unwrap();
    assert_ne!(rx.send_eready, ry.send_eready);
    assert_eq!(rx.send_eready, hash_of(&xp) > hash_of(&yp));
    let (mut snd, mut rcv) = if rx.send_eready {
        (rx.ratchet, ry.ratchet)
    } else {
        (ry.ratchet, rx.ratchet)
    };
    assert!(snd.has_sending_chain() && !rcv.has_sending_chain());
    assert_eq!(snd.associated_data(), rcv.associated_data());
    let len = e2e_enc_agent_msg_length(PqSupport(false));
    let enc = snd.encrypt(b"EREADY", len, None).unwrap();
    assert_eq!(rcv.decrypt(&enc).unwrap(), b"EREADY");
    assert!(
        rcv.has_sending_chain(),
        "the receiving side can answer once it has decrypted"
    );
}

#[test]
fn a_ratchet_key_seen_before_is_ignored_during_and_after_the_resync() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (_, cb) = connect(&mut a, &mut b, false, false);
    let (_, key) = new_key();
    give_key(&mut b, cb, &key, None).unwrap();
    assert_eq!(state(&b, cb), SyncState::Agreed);
    let (agreed, sends) = (ad(&b, cb), pending(&mut b, cb));
    give_key(&mut b, cb, &key, None).unwrap();
    set_state(&mut b, cb, SyncState::Ok);
    give_key(&mut b, cb, &key, None).unwrap();
    assert_eq!(state(&b, cb), SyncState::Ok);
    assert_eq!(ad(&b, cb), agreed);
    assert_eq!(pending(&mut b, cb), sends);
}

/// Opens what `b` queued for `a`'s receive queue.
fn opened(a: &Node, ca: ConnId, p: &PendingSend) -> AgentMsgEnvelope {
    let rq = a.agent.conn(ca).unwrap().rec.rcv.clone().unwrap();
    let env = ClientMsgEnvelope::from_bytes(&p.body).unwrap();
    let secret = dh(rq.peer_e2e.as_ref().unwrap(), &rq.e2e_priv).unwrap();
    let plain = cb_decrypt(&secret, &env.nonce, &env.enc_body).unwrap();
    AgentMsgEnvelope::from_bytes(&ClientMessage::from_bytes(&plain).unwrap().body).unwrap()
}

#[test]
fn our_reply_key_names_the_key_it_answers_at_the_peers_e2e_version() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, cb) = connect(&mut a, &mut b, false, false);
    let (_, key) = new_key();
    let (before, snd_id) = (
        pending_ids(&mut b.store, cb).unwrap(),
        b.agent.conn(cb).unwrap().rec.chain.snd_id,
    );
    give_key(&mut b, cb, &key, None).unwrap();
    let staged: Vec<_> = pending_ids(&mut b.store, cb)
        .unwrap()
        .into_iter()
        .filter(|i| !before.contains(i))
        .collect();
    let rec = b.agent.conn(cb).unwrap().rec.clone();
    assert_eq!(
        rec.chain.snd_id,
        snd_id + i64::try_from(staged.len()).unwrap()
    );
    let p: PendingSend = records::load(&mut b.store, &records::send_key(cb, staged[0]))
        .unwrap()
        .unwrap();
    assert!(p.kind.notifies());
    let AgentMsgEnvelope::RatchetKey {
        agent_version,
        e2e_encryption,
        info,
    } = opened(&a, ca, &p)
    else {
        panic!("not a ratchet key")
    };
    assert_eq!(agent_version, CURRENT_AGENT);
    assert_eq!(e2e_encryption.version, key.params().version);
    assert_eq!(info, ratchet_info(Some(hash_of(&key))).unwrap());
    if staged.len() == 1 {
        assert_eq!(
            rec.chain.snd_hash,
            msg_hash(&info).to_vec(),
            "the next message chains on it"
        );
    }
}

#[test]
fn an_answer_after_the_ratchets_agreed_makes_a_resync_required() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (_, cb) = connect(&mut a, &mut b, false, false);
    let (_, first) = new_key();
    give_key(&mut b, cb, &first, None).unwrap();
    let (_, late) = new_key();
    let err = give_key(&mut b, cb, &late, Some(vec![9; 32])).unwrap_err();
    assert!(matches!(err, AgentError::Prohibited(_)));
    assert_eq!(state(&b, cb), SyncState::Required);
    b.collect();
    assert!(b.events.contains(&AgentEvent::RatchetSync {
        conn: cb,
        state: SyncState::Required
    }));
}

#[test]
fn a_key_answering_a_key_we_did_not_send_is_only_remembered() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (ca, _) = connect(&mut a, &mut b, false, false);
    a.agent.synchronize_ratchet(&mut a.store, ca, true).unwrap();
    let before = ad(&a, ca);
    let (_, other) = new_key();
    give_key(&mut a, ca, &other, Some(vec![9; 32])).unwrap();
    assert_eq!(state(&a, ca), SyncState::Started);
    assert_eq!(ad(&a, ca), before);
    let sync = a.agent.conn(ca).unwrap().rec.sync.clone().unwrap();
    assert!(sync.knows_key(&hash_of(&other)));
}

#[test]
fn a_key_at_an_unsupported_e2e_version_is_refused() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (_, cb) = connect(&mut a, &mut b, false, false);
    let (_, key) = new_key();
    let mut w = wire(&key);
    w.version = Version(9);
    let info = ratchet_info(None).unwrap();
    let r = b
        .agent
        .on_ratchet_key(&mut b.store, cb, Version(8), w, info);
    assert!(matches!(r, Err(AgentError::Version)));
    assert_eq!(state(&b, cb), SyncState::Ok);
}
