//! Joining an address that publishes ratchet keys (agent v8): the request is ratchet-encrypted, and the owner's answer continues that ratchet. The owner is played by the test, which holds the address keys.
use std::time::Duration;

use super::support::{WAIT, rig};
use crate::agent::client::{AgentEvent, ConnId};
use crate::agent::connection::{Confirmation, encrypt_conn_info, from_wire, link_params};
use crate::agent::delivery::{ClientMessage, ClientMsgEnvelope, Opened};
use crate::agent::envelope::AgentMsgEnvelope;
use crate::agent::links::{AddressRatchetKeys, ConnectionLink, ConnectionRequestUri};
use crate::agent::message::AgentMessage;
use crate::agent::records::ConnStatus;
use crate::agent::testing::pump;
use crate::crypto::{cb_decrypt, dh, random_vec};
use crate::encoding::{Decode, Encode, Version};
use crate::ratchet::{
    CURRENT_E2E_VERSION, PqSupport, Ratchet, RatchetVersions, generate_rcv_params, pq_x3dh_rcv,
};

#[test]
fn a_request_to_an_address_with_ratchet_keys_is_encrypted_and_the_answer_continues_its_ratchet() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    // `a` only makes the address queue; it is never stepped, so the request stays on the server.
    let ca: ConnId = a
        .agent
        .create_address(&mut a.store, false, Vec::new())
        .unwrap();
    let ConnectionRequestUri::Contact { data, .. } = pump(&mut [&mut a], WAIT, |n| {
        n[0].take(|e| match e {
            AgentEvent::Address { full, .. } => ConnectionRequestUri::parse(full).ok(),
            _ => None,
        })
    }) else {
        panic!("not an address")
    };
    let rec = a.agent.conn(ca).unwrap().rec.clone();
    let rq = rec.rcv.unwrap();

    let (x3dh, _) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport(true)).unwrap();
    let ratchet_key_id = random_vec(16).unwrap();
    let keys = AddressRatchetKeys {
        ratchet_key_id: ratchet_key_id.clone(),
        e2e: link_params(&x3dh).unwrap(),
    };
    let link = ConnectionRequestUri::Contact {
        data,
        ratchet_keys: Some(keys),
    };
    let cb = b
        .agent
        .join(
            &mut b.store,
            &ConnectionLink::Full(link),
            b"bob's x.contact".to_vec(),
            true,
        )
        .unwrap();
    pump(&mut [&mut b], WAIT, |n| {
        (n[0].agent.conn(cb).unwrap().rec.status == ConnStatus::Invited).then_some(())
    });

    // What the owner finds on the address queue.
    let body = {
        let i = r.smp.lock();
        let q = i.queues.by_rcv.get(&rq.rcv_id).unwrap();
        q.msgs.front().unwrap().1.clone().unwrap()
    };
    let outer = ClientMsgEnvelope::from_bytes(&body).unwrap();
    let secret = dh(outer.e2e_pub.as_ref().unwrap(), &rq.e2e_priv).unwrap();
    let plain =
        ClientMessage::from_bytes(&cb_decrypt(&secret, &outer.nonce, &outer.enc_body).unwrap())
            .unwrap();
    let AgentMsgEnvelope::ContactRequest {
        agent_version,
        e2e_snd_params,
        ratchet_key_id: seen_id,
        enc_conn_info,
    } = AgentMsgEnvelope::from_bytes(&plain.body).unwrap()
    else {
        panic!("not a ratchet-encrypted request")
    };
    assert_eq!(seen_id, ratchet_key_id);
    let snd = from_wire(&e2e_snd_params).unwrap();
    let init = pq_x3dh_rcv(&x3dh, &snd).unwrap();
    let mut owner = Ratchet::init_rcv(
        RatchetVersions::new(snd.version),
        x3dh.pk2.clone(),
        init,
        PqSupport(true),
    );
    let AgentMessage::ConnInfoReply { queues, conn_info } =
        AgentMessage::from_bytes(&owner.decrypt(&enc_conn_info).unwrap()).unwrap()
    else {
        panic!("not a reply")
    };
    assert_eq!(conn_info, b"bob's x.contact");
    let theirs = b.agent.conn(cb).unwrap().rec.rcv.clone().unwrap();
    assert_eq!(queues[0].address.sender_id, theirs.snd_id);

    // The owner answers on the same ratchet, without ratchet params.
    let reply = AgentMessage::ConnInfoReply {
        queues: queues.clone(),
        conn_info: b"alice info".to_vec(),
    }
    .to_bytes()
    .unwrap();
    let env = AgentMsgEnvelope::Confirmation {
        agent_version,
        e2e_encryption: None,
        enc_conn_info: encrypt_conn_info(&mut owner, &reply).unwrap(),
    };
    let Ok(AgentMsgEnvelope::Confirmation { enc_conn_info, .. }) =
        AgentMsgEnvelope::from_bytes(&env.to_bytes().unwrap())
    else {
        panic!()
    };
    let sender = crate::crypto::KeyPair::generate(crate::crypto::Alg::X25519)
        .unwrap()
        .public;
    let opened = Opened {
        version: Version(4),
        sender_e2e: Some(sender.clone()),
        envelope: Vec::new(),
    };
    let conf = Confirmation {
        agent_version,
        e2e: None,
        enc_conn_info,
    };
    b.agent
        .on_confirmation(&mut b.store, cb, &opened, conf)
        .unwrap();
    b.step(Duration::from_millis(1));
    assert!(
        b.events.iter().any(|e| matches!(e, AgentEvent::Conf { conn, conn_info } if *conn == cb && conn_info == b"alice info")),
        "{:?}",
        b.events
    );
    assert_eq!(b.agent.conn(cb).unwrap().rec.status, ConnStatus::Confirmed);
}

#[test]
fn an_address_without_ratchet_keys_refuses_a_ratchet_encrypted_request() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let ca = a
        .agent
        .create_address(&mut a.store, false, Vec::new())
        .unwrap();
    let ConnectionRequestUri::Contact { data, .. } = pump(&mut [&mut a], WAIT, |n| {
        n[0].take(|e| match e {
            AgentEvent::Address { full, .. } => ConnectionRequestUri::parse(full).ok(),
            _ => None,
        })
    }) else {
        panic!("not an address")
    };
    let (x3dh, _) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport(true)).unwrap();
    let keys = AddressRatchetKeys {
        ratchet_key_id: random_vec(16).unwrap(),
        e2e: link_params(&x3dh).unwrap(),
    };
    let link = ConnectionRequestUri::Contact {
        data,
        ratchet_keys: Some(keys),
    };
    b.agent
        .join(
            &mut b.store,
            &ConnectionLink::Full(link),
            b"x".to_vec(),
            true,
        )
        .unwrap();
    pump(&mut [&mut a, &mut b], WAIT, |n| {
        n[0].take(|e| matches!(e, AgentEvent::Err { conn, .. } if *conn == ca).then_some(()))
    });
    assert!(!a.events.iter().any(|e| matches!(e, AgentEvent::Req { .. })));
}
