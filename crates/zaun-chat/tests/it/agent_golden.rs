//! Port of simplexmq's AgentTests/ConnectionRequestTests.hs: its golden strings and binary round trips.
use zaun_chat::agent::links::{
    AddressRatchetKeys, BinaryConnReq, ConnReqUriData, ConnShortLink, ConnectionRequestUri,
    ContactConnType, ServiceScheme, ShortLinkScheme, restore_short_link, shorten_short_link,
};
use zaun_chat::agent::message::{AgentMessage, RatchetInfo};
use zaun_chat::agent::version::{CURRENT_CLIENT, supported_agent_range};
use zaun_chat::agent::{
    E2eParamsUri, KemParams, QueueMode, SmpQueueAddress, SmpQueueInfo, SmpQueueUri, SmpServer,
    url_encode,
};
use zaun_chat::encoding::{Decode, Encode, b64_encode};

use super::agent_support::*;

fn dh_str() -> String {
    b64_encode(&test_dh_key().to_der(), true)
}

const ONION: &str = "jjbyvoemxysm7qxap7m5d5m35jzv5qq6gnlv7s4rsn7tdwwmuqciwpid.onion";

fn addr(server: SmpServer, mode: Option<QueueMode>) -> SmpQueueAddress {
    queue_addr(server, mode)
}

fn no_port(s: SmpServer) -> SmpServer {
    SmpServer { port: None, ..s }
}

fn both<T: PartialEq + std::fmt::Debug>(
    text: &str,
    value: &T,
    parse: impl Fn(&str) -> T,
    print: impl Fn(&T) -> String,
) {
    assert_eq!(print(value), text);
    assert_eq!(&parse(text), value, "parsing {text}");
}

fn dec<T: PartialEq + std::fmt::Debug>(text: &str, value: &T, parse: impl Fn(&str) -> T) {
    assert_eq!(&parse(text), value, "parsing {text}");
}

fn q_both(q: &SmpQueueUri, text: &str) {
    both(
        text,
        q,
        |s| SmpQueueUri::parse(s).unwrap(),
        SmpQueueUri::to_uri,
    );
}

fn q_dec(q: &SmpQueueUri, text: &str) {
    dec(text, q, |s| SmpQueueUri::parse(s).unwrap());
}

fn queue_str(mode_params: &str) -> String {
    format!(
        "smp://1234-w==@smp.simplex.im:5223/3456-w==#/?v=1-4&dh={}{mode_params}&srv={ONION}",
        url_encode(&dh_str())
    )
}

#[test]
fn queue_uris_serialise_and_parse() {
    let plain = queue_supported();
    let supported = zaun_chat::agent::version::supported_client_range();
    let dh = dh_str();
    q_both(&plain, &queue_str("&q=m&k=s"));
    q_dec(
        &plain,
        &format!(
            "smp://1234-w==@smp.simplex.im:5223/3456-w==#{dh}/?v=1-4&extra_param=abc&srv={ONION}&q=m&k=s"
        ),
    );
    q_both(&queue(supported, addr(srv(), None)), &queue_str(""));
    q_both(
        &queue(supported, addr(srv(), Some(QueueMode::Contact))),
        &queue_str("&q=c"),
    );
    let one = queue(supported, addr(srv1(), Some(QueueMode::Messaging)));
    q_both(
        &one,
        &format!(
            "smp://1234-w==@smp.simplex.im:5223/3456-w==#/?v=1-4&dh={}&q=m&k=s",
            url_encode(&dh)
        ),
    );
    let new = queue(
        range(2, CURRENT_CLIENT.0),
        addr(srv(), Some(QueueMode::Messaging)),
    );
    let new_str = format!(
        "smp://1234-w==@smp.simplex.im,{ONION}:5223/3456-w==#/?v=2-4&dh={}&q=m&k=s",
        url_encode(&dh)
    );
    q_both(&new, &new_str);
    q_dec(&new, &new_str.replace(&url_encode(&dh), &dh));
    let new_no_port = queue(
        new.client_v_range,
        addr(no_port(srv()), Some(QueueMode::Messaging)),
    );
    q_both(
        &new_no_port,
        &format!(
            "smp://1234-w==@smp.simplex.im,{ONION}/3456-w==#/?v=2-4&dh={}&q=m&k=s",
            url_encode(&dh)
        ),
    );
    let new1_no_port = queue(
        new.client_v_range,
        addr(no_port(srv1()), Some(QueueMode::Messaging)),
    );
    q_both(
        &new1_no_port,
        &format!(
            "smp://1234-w==@smp.simplex.im/3456-w==#/?v=2-4&dh={}&q=m&k=s",
            url_encode(&dh)
        ),
    );
}

#[test]
fn version_one_queue_uris_use_the_legacy_form() {
    let dh = dh_str();
    let v1 = queue(range(1, 1), addr(srv(), None));
    q_both(
        &v1,
        &format!(
            "smp://1234-w==@smp.simplex.im:5223/3456-w==#/?v=1&dh={}&srv={ONION}",
            url_encode(&dh)
        ),
    );
    q_dec(
        &v1,
        &format!("smp://1234-w==@smp.simplex.im,{ONION}:5223/3456-w==#{dh}"),
    );
    q_dec(
        &v1,
        &format!(
            "smp://1234-w==@smp.simplex.im:5223/3456-w==#/?extra_param=abc&v=1&dh={dh}&srv={ONION}"
        ),
    );
    q_dec(
        &v1,
        &format!(
            "smp://1234-w==@smp.simplex.im:5223/3456-w==#{dh}/?v=1&extra_param=abc&srv={ONION}"
        ),
    );
    let v1_no_port = queue(range(1, 1), addr(no_port(srv()), None));
    let text = format!(
        "smp://1234-w==@smp.simplex.im/3456-w==#/?v=1&dh={}&srv={ONION}",
        url_encode(&dh)
    );
    q_both(&v1_no_port, &text);
    q_dec(&v1_no_port, &text.replace("v=1&", "v=1-1&"));
    q_dec(
        &v1_no_port,
        &format!("smp://1234-w==@smp.simplex.im,{ONION}/3456-w==#{dh}"),
    );
}

fn data(queues: Vec<SmpQueueUri>, vr: zaun_chat::encoding::VersionRange) -> ConnReqUriData {
    ConnReqUriData {
        scheme: ServiceScheme::Simplex,
        agent_v_range: vr,
        smp_queues: queues,
        client_data: None,
    }
}

fn invitation(d: ConnReqUriData) -> ConnectionRequestUri {
    ConnectionRequestUri::Invitation {
        data: d,
        e2e: test_e2e(),
    }
}

fn contact(d: ConnReqUriData) -> ConnectionRequestUri {
    ConnectionRequestUri::Contact {
        data: d,
        ratchet_keys: None,
    }
}

fn r_both(r: &ConnectionRequestUri, text: &str) {
    both(
        text,
        r,
        |s| ConnectionRequestUri::parse(s).unwrap(),
        ConnectionRequestUri::to_uri,
    );
}

fn r_dec(r: &ConnectionRequestUri, text: &str) {
    dec(text, r, |s| {
        ConnectionRequestUri::parse(s).unwrap().into_simplex()
    });
}

const E2E: &str = "v%3D3%26x3dh%3DMEIwBQYDK2VvAzkAmKuSYeQ_m0SixPDS8Wq8VBaTS1cW-Lp0n0h4Diu-kUpR-qXx4SDJ32YGEFoGFGSbGPry5Ychr6U%3D%2CMEIwBQYDK2VvAzkAmKuSYeQ_m0SixPDS8Wq8VBaTS1cW-Lp0n0h4Diu-kUpR-qXx4SDJ32YGEFoGFGSbGPry5Ychr6U%3D";

#[test]
fn e2e_params_print_as_the_golden_query() {
    assert_eq!(url_encode(&test_e2e().to_query()), E2E);
}

#[test]
fn connection_invitations_serialise_and_parse() {
    let supported = zaun_chat::agent::version::supported_client_range();
    let vr = supported_agent_range();
    let q = queue_supported();
    let qs = queue_str("&q=m&k=s");
    let base = invitation(data(vec![q.clone()], vr));
    r_both(
        &base,
        &format!(
            "simplex:/invitation#/?v=6-8&smp={}&e2e={E2E}",
            url_encode(&qs)
        ),
    );
    r_dec(
        &base,
        &format!(
            "https://simplex.chat/invitation#/?v=6-8&smp={}&e2e={E2E}",
            url_encode(&qs)
        ),
    );
    let no_qm = invitation(data(vec![queue(supported, addr(srv(), None))], vr));
    r_both(
        &no_qm,
        &format!(
            "simplex:/invitation#/?v=6-8&smp={}&e2e={E2E}",
            url_encode(&queue_str(""))
        ),
    );
    let two = invitation(data(vec![q.clone(), q.clone()], vr));
    r_both(
        &two,
        &format!(
            "simplex:/invitation#/?v=6-8&smp={}&e2e={E2E}",
            url_encode(&format!("{qs};{qs}"))
        ),
    );
    let mut with_data = data(vec![q.clone()], vr);
    with_data.client_data = Some("{}".into());
    r_both(
        &invitation(with_data),
        &format!(
            "simplex:/invitation#/?v=6-8&smp={}&e2e={E2E}&data={}",
            url_encode(&qs),
            url_encode("{}")
        ),
    );
    let v1 = invitation(data(vec![q], range(1, 1)));
    r_dec(
        &v1,
        &format!(
            "https://simplex.chat/invitation#/?v=1&smp={}&e2e={E2E}",
            url_encode(&qs)
        ),
    );
}

#[test]
fn contact_addresses_serialise_and_parse() {
    let vr = supported_agent_range();
    let q = queue_supported();
    let qs = queue_str("&q=m&k=s");
    let base = contact(data(vec![q.clone()], vr));
    r_both(
        &base,
        &format!("simplex:/contact#/?v=6-8&smp={}", url_encode(&qs)),
    );
    r_dec(
        &base,
        &format!(
            "https://simplex.chat/contact#/?v=6-8&smp={}",
            url_encode(&qs)
        ),
    );
    r_both(
        &contact(data(vec![q.clone(), q.clone()], vr)),
        &format!(
            "simplex:/contact#/?v=6-8&smp={}",
            url_encode(&format!("{qs};{qs}"))
        ),
    );
    let dr = ConnectionRequestUri::Contact {
        data: data(vec![q.clone()], vr),
        ratchet_keys: Some(AddressRatchetKeys {
            ratchet_key_id: b"0123456789abcdef".to_vec(),
            e2e: test_e2e(),
        }),
    };
    r_both(
        &dr,
        &format!(
            "simplex:/contact#/?v=6-8&smp={}&e2e={E2E}&rk=MDEyMzQ1Njc4OWFiY2RlZg%3D%3D",
            url_encode(&qs)
        ),
    );
    let v6 = contact(data(vec![q.clone()], range(6, 6)));
    r_both(
        &v6,
        &format!("simplex:/contact#/?v=6&smp={}", url_encode(&qs)),
    );
    for old in ["1", "1-2", "2-2"] {
        r_dec(
            &v6,
            &format!(
                "https://simplex.chat/contact#/?v={old}&smp={}",
                url_encode(&qs)
            ),
        );
    }
    let mut with_data = data(vec![q], vr);
    let group = r#"{"type":"group_link", "group_link_id":"abc"}"#;
    with_data.client_data = Some(group.into());
    r_both(
        &contact(with_data),
        &format!(
            "simplex:/contact#/?v=6-8&smp={}&data={}",
            url_encode(&qs),
            url_encode(group)
        ),
    );
}

#[test]
fn kem_ciphertext_without_a_kem_key_is_rejected() {
    let text = format!(
        "{}&kem_ct={}",
        test_e2e().to_query(),
        b64_encode(&[0u8; 1039], true)
    );
    assert!(E2eParamsUri::parse(&text).is_err());
}

#[test]
fn queues_and_requests_round_trip_as_binary() {
    let supported = zaun_chat::agent::version::supported_client_range();
    let queue_rt =
        |q: SmpQueueUri| assert_eq!(SmpQueueUri::from_bytes(&q.to_bytes().unwrap()).unwrap(), q);
    queue_rt(queue_supported());
    queue_rt(queue(supported, addr(srv(), None)));
    queue_rt(queue(supported, addr(srv1(), Some(QueueMode::Messaging))));
    queue_rt(queue(range(2, 4), addr(srv(), Some(QueueMode::Messaging))));
    queue_rt(queue(
        range(2, 4),
        addr(no_port(srv1()), Some(QueueMode::Messaging)),
    ));
    queue_rt(queue(range(1, 1), addr(srv(), None)));
    let vr = supported_agent_range();
    let q = queue_supported();
    let binary = |r: ConnectionRequestUri| {
        let b = r.binary();
        assert_eq!(
            BinaryConnReq::from_bytes(&b.to_bytes().unwrap()).unwrap(),
            b
        );
    };
    binary(invitation(data(vec![q.clone()], vr)));
    binary(invitation(data(vec![q.clone(), q.clone()], vr)));
    binary(contact(data(
        vec![queue(supported, addr(srv(), Some(QueueMode::Contact)))],
        vr,
    )));
    binary(contact(data(vec![q.clone()], range(6, 6))));
    let mut with_data = data(vec![q], vr);
    with_data.client_data = Some(r#"{"type":"group_link"}"#.into());
    binary(contact(with_data));
}

fn key32() -> Vec<u8> {
    b"0123456789abcdef0123456789abcdef".to_vec()
}

fn short_srv() -> SmpServer {
    SmpServer::only_host("smp.simplex.im")
}

fn contact_link(scheme: ShortLinkScheme, t: ContactConnType, server: SmpServer) -> ConnShortLink {
    ConnShortLink::Contact {
        scheme,
        contact_type: t,
        server,
        link_key: key32(),
    }
}

fn inv_link(scheme: ShortLinkScheme, server: SmpServer) -> ConnShortLink {
    ConnShortLink::Invitation {
        scheme,
        server,
        link_id: b"0123456789abcdef01234567".to_vec(),
        link_key: key32(),
    }
}

fn s_both(l: &ConnShortLink, text: &str) {
    both(
        text,
        l,
        |s| ConnShortLink::parse(s).unwrap(),
        ConnShortLink::to_uri,
    );
}

#[test]
fn short_links_serialise_and_parse() {
    use ContactConnType::{Contact, Group};
    use ShortLinkScheme::{Server, Simplex};
    let key = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY";
    let id = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3";
    let q = format!("?h={ONION}&p=5223&c=1234-w");
    let qs = format!("?h=smp.simplex.im%2C{ONION}&p=5223&c=1234-w");
    s_both(
        &contact_link(Server, Contact, srv()),
        &format!("https://smp.simplex.im/a#{key}{q}"),
    );
    s_both(
        &contact_link(Server, Group, srv()),
        &format!("https://smp.simplex.im/g#{key}{q}"),
    );
    s_both(
        &contact_link(Server, Contact, short_srv()),
        &format!("https://smp.simplex.im/a#{key}"),
    );
    s_both(
        &inv_link(Server, srv()),
        &format!("https://smp.simplex.im/i#{id}/{key}{q}"),
    );
    s_both(
        &inv_link(Server, short_srv()),
        &format!("https://smp.simplex.im/i#{id}/{key}"),
    );
    s_both(
        &contact_link(Simplex, Contact, srv()),
        &format!("simplex:/a#{key}{qs}"),
    );
    s_both(
        &contact_link(Simplex, Group, srv()),
        &format!("simplex:/g#{key}{qs}"),
    );
    s_both(
        &contact_link(Simplex, Contact, short_srv()),
        &format!("simplex:/a#{key}?h=smp.simplex.im"),
    );
    s_both(
        &inv_link(Simplex, srv()),
        &format!("simplex:/i#{id}/{key}{qs}"),
    );
    s_both(
        &inv_link(Simplex, short_srv()),
        &format!("simplex:/i#{id}/{key}?h=smp.simplex.im"),
    );
}

#[test]
fn preset_servers_shorten_and_restore() {
    use ContactConnType::Contact;
    use ShortLinkScheme::Server;
    let link = |s: SmpServer| contact_link(Server, Contact, s);
    let onion = SmpServer {
        hosts: vec![ONION.into()],
        port: None,
        key_hash: vec![215, 109, 248, 251],
        basic_auth: None,
    };
    let srv2 = SmpServer {
        hosts: vec!["smp2.simplex.im".into(), ONION.into()],
        port: None,
        key_hash: vec![215, 109, 248, 251],
        basic_auth: None,
    };
    let preset = [srv()];
    assert_eq!(shorten_short_link(&preset, link(srv())), link(short_srv()));
    assert_eq!(
        shorten_short_link(&preset, link(onion.clone())),
        link(onion),
        "an onion-only link is not shortened"
    );
    assert_eq!(shorten_short_link(&preset, link(srv1())), link(short_srv()));
    assert_eq!(
        shorten_short_link(&preset, link(srv2.clone())),
        link(srv2.clone())
    );
    assert_eq!(restore_short_link(&preset, link(short_srv())), link(srv()));
    assert_eq!(restore_short_link(&preset, link(srv1())), link(srv1()));
    assert_eq!(
        restore_short_link(std::slice::from_ref(&srv2), link(short_srv())),
        link(short_srv())
    );
    assert_eq!(restore_short_link(&preset, link(srv2.clone())), link(srv2));
}

#[test]
fn localhost_preset_links_shorten_and_restore() {
    let full = "https://localhost/a#4AkRDmhf64tdRlN406g8lJRg5OCmhD6ynIhi6glOcCM?p=7001&c=LcJUMfVhwD8yxjAiSaDzzGF3-kLG4Uh0Fl_ZIjrRwjI";
    let short = "https://localhost/a#4AkRDmhf64tdRlN406g8lJRg5OCmhD6ynIhi6glOcCM";
    let preset =
        [
            SmpServer::parse("smp://LcJUMfVhwD8yxjAiSaDzzGF3-kLG4Uh0Fl_ZIjrRwjI=@localhost:7001")
                .unwrap(),
        ];
    let (l, s) = (
        ConnShortLink::parse(full).unwrap(),
        ConnShortLink::parse(short).unwrap(),
    );
    assert_eq!(shorten_short_link(&preset, l.clone()), s);
    assert_eq!(restore_short_link(&preset, s), l);
    let full = "https://localhost/i#tnUaHYp8saREmyEHR93SBpl8ySHBchOt/LJ1ZQUzxH9Udb0jw5wmJACv5o6oe8e7BsX_hUCUMTSY?p=7001&c=LcJUMfVhwD8yxjAiSaDzzGF3-kLG4Uh0Fl_ZIjrRwjI";
    let short = "https://localhost/i#tnUaHYp8saREmyEHR93SBpl8ySHBchOt/LJ1ZQUzxH9Udb0jw5wmJACv5o6oe8e7BsX_hUCUMTSY";
    let (l, s) = (
        ConnShortLink::parse(full).unwrap(),
        ConnShortLink::parse(short).unwrap(),
    );
    assert_eq!(shorten_short_link(&preset, l.clone()), s);
    assert_eq!(restore_short_link(&preset, s), l);
}

#[test]
fn service_rpc_messages_and_ratchet_info_round_trip() {
    let info = SmpQueueInfo {
        client_version: CURRENT_CLIENT,
        address: queue_addr(srv(), Some(QueueMode::Messaging)),
    };
    let rt =
        |m: AgentMessage| assert_eq!(AgentMessage::from_bytes(&m.to_bytes().unwrap()).unwrap(), m);
    rt(AgentMessage::ServiceRequest {
        queues: vec![info],
        signature: None,
        body: b"service request payload".to_vec(),
    });
    rt(AgentMessage::ServiceResponse(
        b"service response payload".to_vec(),
    ));
    rt(AgentMessage::Rejection(b"rejected: not allowed".to_vec()));
    rt(AgentMessage::RatchetInfo(RatchetInfo {
        answered_key_hash: Some(key32()),
    }));
    let none = AgentMessage::from_bytes(b"R").unwrap();
    assert_eq!(
        none,
        AgentMessage::RatchetInfo(RatchetInfo {
            answered_key_hash: None
        })
    );
    let some = AgentMessage::from_bytes(b"R1\x03abcdef").unwrap();
    assert_eq!(
        some,
        AgentMessage::RatchetInfo(RatchetInfo {
            answered_key_hash: Some(b"abc".to_vec())
        })
    );
}

#[test]
fn proposed_kem_params_round_trip_in_text() {
    let mut e = test_e2e();
    e.kem = Some(KemParams::Proposed { key: vec![1, 2, 3] });
    let text = e.to_query();
    assert_eq!(E2eParamsUri::parse(&text).unwrap(), e);
}
