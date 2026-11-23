//! Port of nook's links.test.ts: frozen links the live CLI printed must re-serialise byte for byte.
use zaun_chat::agent::links::{
    ConnShortLink, ConnectionLink, ConnectionRequestUri, ContactConnType, ShortLinkScheme,
    restore_short_link, shorten_short_link,
};
use zaun_chat::agent::short_link::{contact_short_link_kdf, inv_short_link_kdf};
use zaun_chat::agent::{KemParams, QueueMode, SmpQueueInfo, SmpQueueUri, SmpServer};
use zaun_chat::encoding::{Decode, Encode, Version};

use super::agent_support::*;

fn parsed_invitation() -> ConnectionRequestUri {
    ConnectionRequestUri::parse(CLI_INVITATION_FULL).unwrap()
}

#[test]
fn cli_invitation_reserialises_byte_for_byte() {
    let ConnectionRequestUri::Invitation { data, e2e } = parsed_invitation() else {
        panic!("not an invitation")
    };
    assert_eq!(data.agent_v_range, range(6, 8));
    assert_eq!(data.smp_queues.len(), 1);
    let q = &data.smp_queues[0];
    assert_eq!(q.address.server.hosts.len(), 2);
    assert!(q.address.server.hosts[0].ends_with(".simplex.im"));
    assert!(q.address.server.hosts[1].ends_with(".onion"));
    assert_eq!(q.address.server.key_hash.len(), 32);
    assert_eq!(q.address.sender_id.len(), 24);
    assert_eq!(q.address.dh_public_key.to_der().len(), 44);
    assert_eq!(q.address.queue_mode, Some(QueueMode::Messaging));
    assert_eq!(q.client_v_range, range(1, 4));
    assert_eq!(e2e.v_range, range(3, 3));
    assert_eq!(e2e.key1.to_der().len(), 68);
    assert!(matches!(&e2e.kem, Some(KemParams::Proposed { key }) if key.len() == 1158));
    assert_eq!(parsed_invitation().to_uri(), CLI_INVITATION_FULL);
}

#[test]
fn cli_invitation_binary_forms_round_trip() {
    let ConnectionRequestUri::Invitation { data, e2e } = parsed_invitation() else {
        panic!()
    };
    let qb = data.smp_queues[0].to_bytes().unwrap();
    assert_eq!(
        SmpQueueUri::from_bytes(&qb).unwrap().to_bytes().unwrap(),
        qb
    );
    let eb = e2e.to_bytes().unwrap();
    assert_eq!(
        zaun_chat::agent::E2eParamsUri::from_bytes(&eb).unwrap(),
        e2e
    );
}

#[test]
fn cli_contact_address_reserialises_byte_for_byte() {
    let ConnectionRequestUri::Contact { data, ratchet_keys } = inv(CLI_CONTACT_FULL) else {
        panic!("not a contact")
    };
    assert!(ratchet_keys.is_none());
    assert_eq!(
        data.smp_queues[0].address.queue_mode,
        Some(QueueMode::Contact)
    );
    assert_eq!(data.smp_queues[0].address.sender_id.len(), 24);
    assert_eq!(inv(CLI_CONTACT_FULL).to_uri(), CLI_CONTACT_FULL);
}

#[test]
fn https_form_keeps_its_scheme() {
    let https =
        CLI_INVITATION_FULL.replace("simplex:/invitation", "https://simplex.chat/invitation");
    assert_eq!(inv(&https).to_uri(), https);
    let other = CLI_CONTACT_FULL.replace("simplex:/contact", "https://smp6.simplex.im/contact");
    assert_eq!(inv(&other).to_uri(), other);
    assert_eq!(inv(&https).into_simplex().to_uri(), CLI_INVITATION_FULL);
}

#[test]
fn queue_uri_text_depends_on_the_client_version() {
    let mut q = parsed_invitation().data().smp_queues[0].clone();
    q.client_v_range = range(2, 4);
    let s = q.to_uri();
    assert!(s.contains(&format!("@{}/", q.address.server.hosts.join(","))));
    assert!(!s.contains("srv="));
    assert!(s.contains("&q=m"));
    assert!(
        s.contains("&k=s"),
        "min below 4 still writes the legacy sender-secure parameter"
    );
    q.client_v_range = range(4, 4);
    let s4 = q.to_uri();
    assert!(!s4.contains("k=s"));
    assert!(s4.contains("v=4&"));
}

#[test]
fn queue_info_binary_form_depends_on_the_client_version() {
    let q = parsed_invitation().data().smp_queues[0].clone();
    for v in [2u16, 3, 4] {
        let info = SmpQueueInfo::from_uri(&q, Version(v));
        let back = SmpQueueInfo::from_bytes(&info.to_bytes().unwrap()).unwrap();
        assert_eq!(back.client_version, Version(v));
        assert_eq!(back.address.sender_id, q.address.sender_id);
        let expected = (v >= 3).then_some(QueueMode::Messaging);
        assert_eq!(back.address.queue_mode, expected);
    }
}

#[test]
fn cli_invitation_short_link_reserialises() {
    let l = ConnShortLink::parse(CLI_INVITATION_SHORT).unwrap();
    let ConnShortLink::Invitation {
        scheme,
        server,
        link_id,
        link_key,
    } = &l
    else {
        panic!("not an invitation")
    };
    assert_eq!(*scheme, ShortLinkScheme::Server);
    assert_eq!(server.hosts, ["smp5.simplex.im"]);
    assert!(server.port.is_none() && server.key_hash.is_empty());
    assert_eq!((link_id.len(), link_key.len()), (24, 32));
    assert_eq!(l.to_uri(), CLI_INVITATION_SHORT);
    assert!(matches!(
        ConnectionLink::parse(CLI_INVITATION_SHORT).unwrap(),
        ConnectionLink::Short(_)
    ));
    assert!(matches!(
        ConnectionLink::parse(CLI_INVITATION_FULL).unwrap(),
        ConnectionLink::Full(_)
    ));
}

#[test]
fn cli_contact_short_link_derives_link_id_and_key() {
    let l = ConnShortLink::parse(CLI_CONTACT_SHORT).unwrap();
    let ConnShortLink::Contact {
        contact_type,
        link_key,
        ..
    } = &l
    else {
        panic!("not a contact")
    };
    assert_eq!(
        (*contact_type, link_key.len()),
        (ContactConnType::Contact, 32)
    );
    assert_eq!(l.to_uri(), CLI_CONTACT_SHORT);
    let (link_id, _) = contact_short_link_kdf(link_key).unwrap();
    assert_eq!(link_id.len(), 24);
    assert!(inv_short_link_kdf(link_key).is_ok());
}

#[test]
fn simplex_scheme_and_preset_servers_restore_and_shorten() {
    let l = ConnShortLink::parse(CLI_INVITATION_SHORT).unwrap();
    let preset = SmpServer {
        hosts: vec!["smp5.simplex.im".into()],
        port: None,
        key_hash: vec![7; 32],
        basic_auth: None,
    };
    let restored = restore_short_link(std::slice::from_ref(&preset), l);
    assert_eq!(restored.server().key_hash, preset.key_hash);
    let simplex = restored.clone().into_simplex().to_uri();
    assert!(simplex.starts_with("simplex:/i#"));
    assert!(simplex.contains("?h=smp5.simplex.im&c="));
    assert_eq!(
        ConnShortLink::parse(&simplex).unwrap().server().hosts,
        ["smp5.simplex.im"]
    );
    assert!(
        shorten_short_link(&[preset], restored)
            .server()
            .key_hash
            .is_empty()
    );
}
