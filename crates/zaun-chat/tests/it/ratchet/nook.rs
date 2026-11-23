//! Known answers from nook's ratchet (made by gen_nook_transcript.ts): params, X3DH output, and a sender's first messages must match byte for byte; its messages must decrypt here, in and out of order.
use crate::hex::unhex;
use serde_json::Value;
use zaun_chat::crypto::{Alg, PrivateKey};
use zaun_chat::encoding::{Decode, Encode};
use zaun_chat::ratchet::{
    CURRENT_E2E_VERSION, E2eParams, KemCiphertext, KemKeyPair, KemPublicKey, KemSecretKey,
    KemSharedSecret, ParamsUri, PqSupport, PrivKem, PrivParams, Ratchet, RatchetVersions,
    RcvParams, pq_x3dh_rcv, pq_x3dh_snd,
};

const TRANSCRIPT: &str = include_str!("nook_transcript.json");

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap()
}

fn kem_pair(v: &Value) -> KemKeyPair {
    KemKeyPair {
        public: KemPublicKey::from_bytes(&unhex(s(v, "pk"))).unwrap(),
        secret: KemSecretKey::from_bytes(&unhex(s(v, "sk"))).unwrap(),
    }
}

fn priv_params(v: &Value) -> PrivParams {
    let key = |k| PrivateKey::from_raw(Alg::X448, &unhex(s(v, k))).unwrap();
    let kem = match &v["kem"] {
        Value::Null => None,
        k if s(k, "tag") == "P" => Some(PrivKem::Proposed(kem_pair(&k["kp"]))),
        k => Some(PrivKem::Accepted {
            ct: KemCiphertext::from_bytes(&unhex(s(k, "ct"))).unwrap(),
            shared: KemSharedSecret::from_bytes(&unhex(s(k, "ss"))).unwrap(),
            keys: kem_pair(&k["kp"]),
        }),
    };
    PrivParams {
        pk1: key("pk1"),
        pk2: key("pk2"),
        kem,
    }
}

fn check(case: &Value) {
    let pq = case["pq"].as_bool().unwrap();
    let (alice, bob) = (priv_params(&case["alice"]), priv_params(&case["bob"]));
    let rcv = RcvParams::from_bytes(&unhex(s(case, "aliceParams"))).unwrap();
    assert_eq!(rcv.to_bytes().unwrap(), unhex(s(case, "aliceParams")));
    assert_eq!(alice.rcv_params(CURRENT_E2E_VERSION).unwrap(), rcv);
    let snd = E2eParams::from_bytes(&unhex(s(case, "bobParams"))).unwrap();
    assert_eq!(snd.to_bytes().unwrap(), unhex(s(case, "bobParams")));
    let uri = ParamsUri::from_bytes(&unhex(s(case, "uriBin"))).unwrap();
    assert_eq!(uri.to_bytes().unwrap(), unhex(s(case, "uriBin")));
    assert_eq!(uri.to_query(), s(case, "uriQuery"));
    assert_eq!(ParamsUri::from_query(s(case, "uriQuery")).unwrap(), uri);

    let (bob_init, bob_kem) = pq_x3dh_snd(&bob, &rcv).unwrap();
    let (alice_init, alice_kem) = pq_x3dh_rcv(&alice, &snd).unwrap();
    super::support::assert_same_init(&alice_init, &bob_init);
    let init = &case["init"];
    assert_eq!(bob_init.assoc_data, unhex(s(init, "ad")));
    assert_eq!(bob_init.verify_code_pq.to_vec(), unhex(s(init, "vc")));
    assert_eq!(
        bob_init.ratchet_key.as_bytes().to_vec(),
        unhex(s(init, "rk"))
    );
    assert_eq!(bob_init.snd_hk.as_bytes().to_vec(), unhex(s(init, "sndHK")));
    assert_eq!(
        bob_init.rcv_next_hk.as_bytes().to_vec(),
        unhex(s(init, "rcvNextHK"))
    );

    let versions = RatchetVersions::new(CURRENT_E2E_VERSION);
    let dhrs = PrivateKey::from_raw(Alg::X448, &unhex(s(&case["bob"], "dhrs"))).unwrap();
    let mut bob_rc =
        Ratchet::init_snd(versions, rcv.params().k2.clone(), dhrs, (bob_init, bob_kem)).unwrap();
    let texts = case["texts"].as_array().unwrap();
    let msgs = case["msgs"].as_array().unwrap();
    for (t, m) in texts.iter().zip(msgs) {
        let wire = bob_rc
            .encrypt(t.as_str().unwrap().as_bytes(), 100, None)
            .unwrap();
        assert_eq!(
            wire,
            unhex(m.as_str().unwrap()),
            "sender output equals nook's"
        );
    }

    let mut alice_rc = Ratchet::init_rcv(
        versions,
        alice.pk2.clone(),
        (alice_init, alice_kem),
        PqSupport(pq),
    );
    for i in [0, 3, 1, 5, 2, 4] {
        let plain = alice_rc.decrypt(&unhex(msgs[i].as_str().unwrap())).unwrap();
        assert_eq!(plain, texts[i].as_str().unwrap().as_bytes());
    }
    assert_eq!(alice_rc.skipped_len(), 0);
}

#[test]
fn params_x3dh_and_first_messages_match_nook_without_pq() {
    let t: Value = serde_json::from_str(TRANSCRIPT).unwrap();
    check(&t["off"]);
}

#[test]
fn params_x3dh_and_first_messages_match_nook_with_an_accepted_kem() {
    let t: Value = serde_json::from_str(TRANSCRIPT).unwrap();
    check(&t["on"]);
}
