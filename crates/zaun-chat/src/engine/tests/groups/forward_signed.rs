//! Signed (`/`) elements in a classic group: optional, but checked against the author's key when it is known (Subscriber.hs `withVerifiedMsg`, `verifyGroupSig`).

use zaun_chat_wire::model::MessageAuthor;

use super::super::{T, messages};
use super::forward::{forwarded, from_host, fwd, json};
use super::messages::{member_id, one};
use crate::chat::ChatEvent;
use crate::chat::container::MsgContainer;
use crate::chat::element::encode_signed_element;
use crate::chat::group::GroupMemberRole;
use crate::chat::ids::{MemberId, b64_encode};
use crate::chat::signed::{ChatBinding, MsgSignature, SignedMsg};
use crate::crypto::{Alg, KeyPair};

/// Gives member `member` a fresh key: the pair, to sign with.
fn keyed(t: &mut T, g: i64, member: i64) -> KeyPair {
    let kp = KeyPair::generate(Alg::Ed25519).unwrap();
    let mut m = t.e.state.members[&(g, member)].clone();
    m.key = Some(b64_encode(&kp.public.to_der()));
    t.e.save_member(m).unwrap();
    kp
}

/// `body` signed by `kp` as `author` would (`'G' ‖ memberId ‖ key DER ‖ body`).
fn signed(kp: &KeyPair, author: &MemberId, body: Vec<u8>) -> Vec<u8> {
    let der = kp.public.to_der();
    let mut data = vec![u8::try_from(author.0.len()).unwrap()];
    data.extend_from_slice(&author.0);
    data.push(u8::try_from(der.len()).unwrap());
    data.extend_from_slice(&der);
    let mut sm = SignedMsg {
        binding: ChatBinding::Group,
        signatures: Vec::new(),
        signed_body: body,
    };
    let sig = kp.private.sign(&sm.signed_input(&data)).unwrap();
    sm.signatures.push(MsgSignature {
        signature: sig.as_bytes().to_vec(),
    });
    encode_signed_element(&sm).unwrap()
}

fn text(s: &str) -> Vec<u8> {
    json(ChatEvent::MsgNew(MsgContainer::text(s)))
}

#[test]
fn a_signature_from_a_member_whose_key_is_known_must_verify_or_the_message_is_dropped() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let (carol, conn) = ms[1];
    let kp = keyed(&mut t, g, carol);
    let who = member_id(&t, g, carol);
    let good = signed(&kp, &who, text("signed"));
    let m = one(&from_host(&mut t, &conn, 1, good));
    assert_eq!(m.author, MessageAuthor::Member("carol".into()));

    // `/`, binding, count, `M`, length, then the 64 signature bytes.
    let mut bad = signed(&kp, &who, text("tampered"));
    bad[10] ^= 1;
    assert!(messages(&from_host(&mut t, &conn, 2, bad)).is_empty());
    let other = KeyPair::generate(Alg::Ed25519).unwrap();
    let forged = signed(&other, &who, text("forged"));
    assert!(messages(&from_host(&mut t, &conn, 3, forged)).is_empty());
}

#[test]
fn an_unsigned_message_or_one_without_a_known_key_is_taken_in_a_classic_group() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    keyed(&mut t, g, ms[1].0);
    one(&from_host(&mut t, &ms[1].1, 1, text("plain")));
    let stranger = KeyPair::generate(Alg::Ed25519).unwrap();
    let bob = member_id(&t, g, ms[0].0);
    let body = signed(&stranger, &bob, text("no key for bob here"));
    one(&from_host(&mut t, &ms[0].1, 2, body));
}

#[test]
fn a_forwarded_signature_is_checked_with_the_authors_key_not_the_forwarders() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Member);
    let (host, carol) = (ms[0], ms[1]);
    keyed(&mut t, g, host.0);
    let kp = keyed(&mut t, g, carol.0);
    let who = member_id(&t, g, carol.0);
    let f = fwd(who.clone(), "carol");
    let good = forwarded(&f, &signed(&kp, &who, text("via the host")));
    let m = one(&from_host(&mut t, &host.1, 1, good));
    assert_eq!(m.author, MessageAuthor::Member("carol".into()));
    let other = KeyPair::generate(Alg::Ed25519).unwrap();
    let bad = forwarded(&f, &signed(&other, &who, text("not carol's")));
    assert!(messages(&from_host(&mut t, &host.1, 2, bad)).is_empty());
}

#[test]
fn a_signed_element_an_admin_passes_on_keeps_its_bytes_and_still_verifies() {
    let mut t = T::started();
    let (g, ms) = t.group("team", &["bob", "carol"], GroupMemberRole::Admin);
    let (bob, carol) = (ms[0], ms[1]);
    let kp = keyed(&mut t, g, bob.0);
    let who = member_id(&t, g, bob.0);
    let el = signed(&kp, &who, text("signed by bob"));
    let p = crate::chat::element::parse_element(&el).unwrap();
    let relay = t.e.relay_element(g, bob.0, &p).unwrap();
    t.e.forward_to(g, &[carol.0], &[relay]).unwrap();
    let (_, body) = t.e.agent.sent().pop().unwrap();
    let mut got = crate::chat::parse_chat_elements(&body);
    let got = got.remove(0).unwrap();
    let sm = got.signed.unwrap();
    assert_eq!(
        sm,
        p.signed.unwrap(),
        "the signature and the signed bytes as they came"
    );
    let key = t.e.state.members[&(g, bob.0)].key.clone().unwrap();
    assert!(crate::engine::groups::verify::signed_by(&who, &key, &sm));
}
