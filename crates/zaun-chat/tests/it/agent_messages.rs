//! Port of nook's messages.test.ts, and a round trip of every envelope and agent message.
use zaun_chat::agent::links::ConnectionRequestUri;
use zaun_chat::agent::message::{
    AMessage, AMessageReceipt, APrivHeader, AgentMessage, AgentMessageType, RatchetInfo,
    RequestSignature, SndQAddr,
};
use zaun_chat::agent::version::{
    CURRENT_AGENT, fast_rotation, min_client_version, supported_agent_range,
};
use zaun_chat::agent::{
    AgentMsgEnvelope, E2eParams, KemParams, MsgErrorType, MsgIntegrity, QueueMode, SmpQueueInfo,
    SmpQueueUri, check_msg_integrity, msg_hash,
};
use zaun_chat::crypto::{Alg, KeyPair};
use zaun_chat::encoding::{Decode, Encode, Version};

use super::agent_support::*;

fn queue() -> SmpQueueUri {
    inv(CLI_INVITATION_FULL).data().smp_queues[0].clone()
}

fn info() -> SmpQueueInfo {
    SmpQueueInfo::from_uri(&queue(), Version(4))
}

fn addr() -> SndQAddr {
    let q = queue();
    SndQAddr {
        server: q.address.server,
        sender_id: q.address.sender_id,
    }
}

fn rt<T: Encode + Decode + PartialEq + std::fmt::Debug>(v: T) -> Vec<u8> {
    let b = v.to_bytes().unwrap();
    let back = T::from_bytes(&b).unwrap();
    assert_eq!(back, v);
    assert_eq!(back.to_bytes().unwrap(), b);
    b
}

fn ed_key() -> KeyPair {
    KeyPair::generate(Alg::Ed25519).unwrap()
}

#[test]
fn every_amessage_round_trips() {
    let receipt = AMessageReceipt {
        agent_msg_id: 5,
        msg_hash: vec![0; 32],
        rcpt_info: vec![9],
    };
    rt(AMessage::Hello);
    rt(AMessage::AMsg(vec![1, 2, 3]));
    rt(AMessage::ARcvd(vec![receipt.clone(), receipt]));
    rt(AMessage::AQcont(addr()));
    rt(AMessage::Qadd(vec![
        (queue(), Some(addr())),
        (queue(), None),
    ]));
    rt(AMessage::Qkey(vec![(info(), ed_key().public)]));
    rt(AMessage::Quse(vec![(addr(), true), (addr(), false)]));
    rt(AMessage::Qtest(vec![addr()]));
    rt(AMessage::Qend(vec![addr()]));
    rt(AMessage::Eready(12));
}

#[test]
fn amessage_tags_are_the_wire_letters() {
    assert_eq!(AMessage::Hello.to_bytes().unwrap(), b"H");
    assert_eq!(AMessage::AMsg(vec![0x7b]).to_bytes().unwrap(), [b'M', 0x7b]);
    assert_eq!(
        AMessage::Eready(12).to_bytes().unwrap(),
        [b'E', 0, 0, 0, 0, 0, 0, 0, 12]
    );
    for (m, tag) in [
        (AMessage::AQcont(addr()), "QC"),
        (AMessage::Qadd(vec![(queue(), None)]), "QA"),
        (AMessage::Qtest(vec![addr()]), "QT"),
        (AMessage::Qend(vec![addr()]), "QE"),
    ] {
        assert!(m.to_bytes().unwrap().starts_with(tag.as_bytes()));
        assert_eq!(m.message_type().tag(), tag);
    }
}

#[test]
fn empty_lists_are_not_encodable_or_decodable() {
    assert!(AMessage::Qend(vec![]).to_bytes().is_err());
    assert!(AMessage::from_bytes(b"QE\x00").is_err());
    assert!(AMessage::from_bytes(b"QZ").is_err());
}

#[test]
fn conn_info_reply_carries_the_reply_queue_and_chat_info() {
    let m = AgentMessage::ConnInfoReply {
        queues: vec![info()],
        conn_info: br#"{"event":"x.info"}"#.to_vec(),
    };
    let b = rt(m);
    assert_eq!(b[0], b'D');
    let q = SmpQueueInfo::from_bytes(&{
        let mut d = zaun_chat::encoding::Decoder::new(&b[2..]);
        let v = d.take_all().to_vec();
        v[..v.len() - br#"{"event":"x.info"}"#.len()].to_vec()
    })
    .unwrap();
    assert_eq!(q.client_version, Version(4));
}

#[test]
fn agent_messages_round_trip_with_their_header() {
    rt(AgentMessage::ConnInfo(vec![1]));
    rt(AgentMessage::RatchetInfo(RatchetInfo {
        answered_key_hash: Some(vec![3; 32]),
    }));
    let sig = ed_key();
    let signature = RequestSignature {
        key: sig.public.clone(),
        sig: sig.private.sign(b"x").unwrap(),
    };
    rt(AgentMessage::ServiceRequest {
        queues: vec![info()],
        signature: Some(signature),
        body: vec![1, 2],
    });
    let header = APrivHeader {
        snd_msg_id: 2,
        prev_msg_hash: msg_hash(&[1]).to_vec(),
    };
    let bytes = rt(AgentMessage::Message {
        header,
        message: AMessage::Hello,
    });
    assert_eq!(
        bytes.len(),
        1 + 8 + 1 + 32 + 1,
        "M | int64 id | len 32 | hash | H"
    );
    assert_eq!(
        AgentMessage::from_bytes(&bytes).unwrap().message_type(),
        AgentMessageType::Hello
    );
}

fn e2e_snd() -> E2eParams {
    let ConnectionRequestUri::Invitation { e2e, .. } = inv(CLI_INVITATION_FULL) else {
        panic!()
    };
    E2eParams {
        version: Version(3),
        key1: e2e.key1,
        key2: e2e.key2,
        kem: Some(KemParams::Accepted {
            ct: vec![0; 10],
            key: vec![1; 12],
        }),
    }
}

fn e2e_rcv() -> E2eParams {
    E2eParams {
        kem: Some(KemParams::Proposed { key: vec![2; 20] }),
        ..e2e_snd()
    }
}

#[test]
fn every_envelope_round_trips() {
    let v = Version(8);
    let conf = rt(AgentMsgEnvelope::Confirmation {
        agent_version: v,
        e2e_encryption: Some(e2e_snd()),
        enc_conn_info: vec![5, 6],
    });
    assert_eq!(&conf[..3], [0, 8, b'C']);
    rt(AgentMsgEnvelope::Confirmation {
        agent_version: v,
        e2e_encryption: None,
        enc_conn_info: vec![1],
    });
    rt(AgentMsgEnvelope::Message {
        agent_version: v,
        enc_agent_message: vec![1, 2],
    });
    rt(AgentMsgEnvelope::Invitation {
        agent_version: v,
        conn_req: inv(CLI_INVITATION_FULL),
        conn_info: vec![7],
    });
    rt(AgentMsgEnvelope::ContactRequest {
        agent_version: v,
        e2e_snd_params: e2e_snd(),
        ratchet_key_id: vec![0; 16],
        enc_conn_info: vec![1],
    });
    rt(AgentMsgEnvelope::RatchetKey {
        agent_version: v,
        e2e_encryption: e2e_rcv(),
        info: vec![1],
    });
}

/// `SndE2ERatchetParams` takes any KEM state: a joiner with PQ proposes a KEM to a link without one (seen from the 7.1.4 CLI).
#[test]
fn a_confirmation_takes_any_kem_state_but_a_ratchet_key_only_a_proposal() {
    let v = Version(8);
    let proposing = AgentMsgEnvelope::Confirmation {
        agent_version: v,
        e2e_encryption: Some(e2e_rcv()),
        enc_conn_info: vec![],
    };
    let bytes = proposing.to_bytes().unwrap();
    assert_eq!(AgentMsgEnvelope::from_bytes(&bytes).unwrap(), proposing);
    let bad_key = AgentMsgEnvelope::RatchetKey {
        agent_version: v,
        e2e_encryption: e2e_snd(),
        info: vec![],
    };
    assert!(AgentMsgEnvelope::from_bytes(&bad_key.to_bytes().unwrap()).is_err());
}

#[test]
fn an_invitation_envelope_needs_an_invitation_link() {
    let contact = AgentMsgEnvelope::Invitation {
        agent_version: Version(8),
        conn_req: inv(CLI_CONTACT_FULL),
        conn_info: vec![],
    };
    assert!(AgentMsgEnvelope::from_bytes(&contact.to_bytes().unwrap()).is_err());
}

#[test]
fn message_integrity_follows_the_chain() {
    let h = [1u8; 32];
    use MsgErrorType::{BadHash, BadId, Duplicate, Skipped};
    assert_eq!(check_msg_integrity(1, 2, &h, &h), MsgIntegrity::Ok);
    assert_eq!(
        check_msg_integrity(1, 4, &h, &h),
        MsgIntegrity::Error(Skipped { from: 2, to: 3 })
    );
    assert_eq!(
        check_msg_integrity(2, 2, &h, &h),
        MsgIntegrity::Error(Duplicate)
    );
    assert_eq!(
        check_msg_integrity(2, 1, &h, &h),
        MsgIntegrity::Error(BadId(1))
    );
    assert_eq!(
        check_msg_integrity(1, 2, &h, &[0; 32]),
        MsgIntegrity::Error(BadHash)
    );
    assert_eq!(check_msg_integrity(0, 1, &[], &[]), MsgIntegrity::Ok);
    assert_eq!(
        MsgIntegrity::Error(Skipped { from: 2, to: 3 }).to_string(),
        "ERR NO_ID 2 3"
    );
    assert_eq!(MsgIntegrity::Ok.to_string(), "OK");
}

#[test]
fn version_gates_follow_the_agent_version() {
    assert_eq!(supported_agent_range().max(), CURRENT_AGENT);
    assert!(fast_rotation(Version(8)) && !fast_rotation(Version(7)));
    assert_eq!(min_client_version(Version(8)), Version(4));
    assert_eq!(min_client_version(Version(7)), Version(3));
    assert_eq!(min_client_version(Version(6)), Version(1));
    assert_eq!(queue().address.queue_mode, Some(QueueMode::Messaging));
}
