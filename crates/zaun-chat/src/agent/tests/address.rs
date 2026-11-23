//! Contact addresses against the in-memory server: making one, requests through it, accepting, rejecting, deleting.
use super::support::{Rig, WAIT, next_msg, rig, wait_for};
use crate::agent::client::{AgentEvent, ConnId};
use crate::agent::links::ConnectionLink;
use crate::agent::testing::{Node, pump};

/// `a` makes an address: its connection and (full, short) links.
fn address(a: &mut Node, short: bool) -> (ConnId, String, Option<String>) {
    let conn = a
        .agent
        .create_address(&mut a.store, short, b"alice's card".to_vec())
        .unwrap();
    let (full, short) = pump(&mut [&mut *a], WAIT, |n| {
        n[0].take(|e| match e {
            AgentEvent::Address { full, short, .. } => Some((full.clone(), short.clone())),
            _ => None,
        })
    });
    (conn, full, short)
}

/// `b` asks the address behind `link`; `a` hears of it: the request's id and what `b` sent.
fn request(a: &mut Node, b: &mut Node, link: &str, pq: bool) -> (ConnId, Vec<u8>, Vec<u8>) {
    let link = ConnectionLink::parse(link).unwrap();
    let cb = b
        .agent
        .join(&mut b.store, &link, b"bob's x.contact".to_vec(), pq)
        .unwrap();
    let (inv, info) = wait_for(&mut [&mut *a, &mut *b], 0, |e| match e {
        AgentEvent::Req {
            inv_id, conn_info, ..
        } => Some((inv_id.clone(), conn_info.clone())),
        _ => None,
    });
    (cb, inv, info)
}

/// The rest of the handshake after `a` accepted `inv`: both sides are connected.
fn connected(r: &Rig, a: &mut Node, b: &mut Node, inv: &[u8], cb: ConnId) -> ConnId {
    let _ = r;
    let ca = a
        .agent
        .accept_contact(&mut a.store, inv, b"alice info".to_vec(), true)
        .unwrap();
    let conf = wait_for(&mut [&mut *a, &mut *b], 1, |e| match e {
        AgentEvent::Conf { conn, conn_info } if *conn == cb => Some(conn_info.clone()),
        _ => None,
    });
    assert_eq!(conf, b"alice info");
    b.agent
        .allow(&mut b.store, cb, b"bob info".to_vec())
        .unwrap();
    let info = wait_for(&mut [&mut *a, &mut *b], 0, |e| match e {
        AgentEvent::Info { conn, conn_info } if *conn == ca => Some(conn_info.clone()),
        _ => None,
    });
    assert_eq!(info, b"bob info");
    pump(&mut [&mut *a, &mut *b], WAIT, |n| {
        let up = |n: &Node, c| n.events.contains(&AgentEvent::Con { conn: c });
        (up(n[0], ca) && up(n[1], cb)).then_some(())
    });
    ca
}

#[test]
fn an_address_is_made_with_a_short_link_and_a_full_one() {
    let r = rig();
    let mut a = r.node("a");
    let (_, full, short) = address(&mut a, true);
    assert!(full.starts_with("simplex:/contact#/?v=6-8&smp="), "{full}");
    assert!(full.ends_with("%26q%3Dc"), "a contact queue: {full}");
    let short = short.unwrap();
    assert!(short.contains("/a#"), "{short}");
    assert!(ConnectionLink::parse(&short).is_ok());
    assert_eq!(r.smp.count("NEW"), 1);
}

#[test]
fn a_group_address_carries_its_client_data_and_a_g_short_link_a_joiner_can_use() {
    use crate::agent::links::ConnectionRequestUri;
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let data = r#"{"groupLinkId":"AAAAAAAAAAAAAAAAAAAAAA=="}"#;
    a.agent
        .create_address_with(&mut a.store, true, b"group".to_vec(), Some(data.into()))
        .unwrap();
    let (full, short) = pump(&mut [&mut a], WAIT, |n| {
        n[0].take(|e| match e {
            AgentEvent::Address { full, short, .. } => Some((full.clone(), short.clone())),
            _ => None,
        })
    });
    let ConnectionLink::Full(ConnectionRequestUri::Contact { data: d, .. }) =
        ConnectionLink::parse(&full).unwrap()
    else {
        panic!("{full}")
    };
    assert_eq!(d.client_data.as_deref(), Some(data));
    let short = short.unwrap();
    assert!(short.contains("/g#"), "{short}");
    // The short link resolves to the same request, client data and all.
    let (_, _, info) = request(&mut a, &mut b, &short, false);
    assert_eq!(info, b"bob's x.contact");
}

#[test]
fn an_address_without_a_short_link_gives_only_the_full_one() {
    let r = rig();
    let mut a = r.node("a");
    let (_, full, short) = address(&mut a, false);
    assert!(full.starts_with("simplex:/contact#"));
    assert_eq!(short, None);
}

#[test]
fn a_request_through_the_short_link_is_accepted_and_the_two_exchange_messages() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (_, _, short) = address(&mut a, true);
    let (cb, inv, info) = request(&mut a, &mut b, &short.unwrap(), true);
    assert_eq!(info, b"bob's x.contact");
    assert_eq!((r.smp.count("LGET"), r.smp.count("LKEY")), (1, 0));
    let ca = connected(&r, &mut a, &mut b, &inv, cb);
    a.agent.send(&mut a.store, ca, b"hello".to_vec()).unwrap();
    let (id, body) = next_msg(&mut [&mut a, &mut b], 1, cb);
    assert_eq!(body, b"hello");
    b.agent.ack(&mut b.store, cb, id, None).unwrap();
}

#[test]
fn a_request_through_the_full_link_needs_no_lookup() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (_, full, _) = address(&mut a, true);
    let (cb, inv, _) = request(&mut a, &mut b, &full, false);
    assert_eq!(r.smp.count("LGET"), 0);
    connected(&r, &mut a, &mut b, &inv, cb);
}

#[test]
fn a_rejected_request_sends_nothing_and_cannot_be_answered_again() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (_, _, short) = address(&mut a, true);
    let (_, inv, _) = request(&mut a, &mut b, &short.unwrap(), true);
    let sends = r.smp.count("SEND");
    a.agent.reject_contact(&mut a.store, &inv).unwrap();
    a.step(std::time::Duration::from_millis(20));
    assert_eq!(r.smp.count("SEND"), sends, "the requester is not told");
    assert!(a.agent.reject_contact(&mut a.store, &inv).is_err());
    assert!(
        a.agent
            .accept_contact(&mut a.store, &inv, vec![], true)
            .is_err()
    );
}

#[test]
fn a_request_for_a_deleted_address_fails_for_the_requester() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (conn, _, short) = address(&mut a, true);
    a.agent.delete(&mut a.store, conn).unwrap();
    a.step(std::time::Duration::from_millis(20));
    let link = ConnectionLink::parse(&short.unwrap()).unwrap();
    let cb = b
        .agent
        .join(&mut b.store, &link, b"x".to_vec(), true)
        .unwrap();
    wait_for(&mut [&mut a, &mut b], 1, |e| {
        matches!(e, AgentEvent::Err { conn, .. } if *conn == cb).then_some(())
    });
}

#[test]
fn requests_wait_through_a_restart_and_are_answered_afterwards() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (_, _, short) = address(&mut a, true);
    let (cb, inv, _) = request(&mut a, &mut b, &short.unwrap(), true);
    drop(a);
    let mut a = r.node("a");
    connected(&r, &mut a, &mut b, &inv, cb);
}

#[test]
fn deleting_the_address_forgets_its_waiting_requests() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (conn, _, short) = address(&mut a, true);
    let (_, inv, _) = request(&mut a, &mut b, &short.unwrap(), true);
    a.agent.delete(&mut a.store, conn).unwrap();
    assert!(
        a.agent
            .accept_contact(&mut a.store, &inv, vec![], true)
            .is_err()
    );
}

#[test]
fn an_address_keeps_its_data_signed_by_a_root_key_whose_hash_is_the_entity_id() {
    use crate::agent::links::{BinaryConnReq, ConnShortLink, ConnectionMode, ContactConnType};
    use crate::agent::short_link::{
        ConnLinkData, EncLinkData, contact_short_link_kdf, decrypt_link_data,
    };
    use crate::crypto::sha256;

    let r = rig();
    let mut a = r.node("a");
    let (_, _, short) = address(&mut a, true);
    let Ok(ConnectionLink::Short(ConnShortLink::Contact {
        contact_type,
        link_key,
        ..
    })) = ConnectionLink::parse(&short.unwrap())
    else {
        panic!("not a contact short link")
    };
    assert_eq!(contact_type, ContactConnType::Contact);
    let (link_id, key) = contact_short_link_kdf(&link_key).unwrap();
    let stored = {
        let i = r.smp.lock();
        let q = i
            .queues
            .by_rcv
            .get(i.queues.by_link.get(&link_id).unwrap())
            .unwrap();
        assert!(q.contact, "NEW asked for a contact queue");
        q.link.clone().unwrap().1
    };
    let enc = EncLinkData {
        fixed: stored.fixed,
        user: stored.user,
    };
    let (fixed, user) = decrypt_link_data(ConnectionMode::Contact, &link_key, &key, &enc).unwrap();
    assert_eq!(
        fixed.link_entity_id,
        Some(sha256(fixed.root_key.as_bytes()).to_vec())
    );
    let BinaryConnReq::Contact { data } = fixed.link_conn_req else {
        panic!("not an address")
    };
    assert_eq!(
        data.smp_queues[0].address.queue_mode,
        Some(crate::agent::QueueMode::Contact)
    );
    let ConnLinkData::Contact { data: owner, .. } = user else {
        panic!("not an address")
    };
    assert!(owner.direct && owner.owners.is_empty() && owner.relays.is_empty());
    assert!(
        owner.ratchet_keys.is_none(),
        "the apps' default: plain requests"
    );
    assert_eq!(owner.user_data.0, b"alice's card");
}

#[test]
fn a_request_made_while_the_relay_is_unreachable_goes_when_it_is_back() {
    let r = rig();
    let (mut a, mut b) = (r.node("a"), r.node("b"));
    let (_, full, _) = address(&mut a, false);
    r.smp.refuse(true);
    let link = ConnectionLink::parse(&full).unwrap();
    let cb = b
        .agent
        .join(&mut b.store, &link, b"bob's x.contact".to_vec(), true)
        .unwrap();
    for _ in 0..20 {
        b.step(std::time::Duration::from_millis(2));
    }
    assert!(
        a.events
            .iter()
            .all(|e| !matches!(e, AgentEvent::Req { .. }))
    );
    r.smp.refuse(false);
    let inv = wait_for(&mut [&mut a, &mut b], 0, |e| match e {
        AgentEvent::Req { inv_id, .. } => Some(inv_id.clone()),
        _ => None,
    });
    connected(&r, &mut a, &mut b, &inv, cb);
}
