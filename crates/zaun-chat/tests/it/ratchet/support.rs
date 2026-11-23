//! A full handshake as the agent does it, through the wire encodings, and send/receive helpers.
use zaun_chat::crypto::{Alg, KeyPair, PublicKey};
use zaun_chat::encoding::{Decode, Encode};
use zaun_chat::ratchet::{
    CURRENT_E2E_VERSION, E2eParams, InitParams, ParamsUri, PqSupport, Ratchet, RatchetError,
    RatchetVersions, compatible_e2e_version, generate_rcv_params, generate_snd_params, pq_x3dh_rcv,
    pq_x3dh_snd, reply_kem, supported_e2e_range,
};

pub const PADDED: usize = 100;

pub struct Pair {
    /// Created the invitation; receives first.
    pub alice: Ratchet,
    /// Joined; sends first.
    pub bob: Ratchet,
    pub alice_init: InitParams,
    pub bob_init: InitParams,
    pub bob_dhrs: PublicKey,
}

pub fn handshake(invite_pq: bool, join_pq: bool) -> Pair {
    let (a_priv, a_params) =
        generate_rcv_params(CURRENT_E2E_VERSION, PqSupport(invite_pq)).unwrap();
    let uri = ParamsUri::from_params(&a_params, supported_e2e_range());
    let uri = {
        let parsed = ParamsUri::from_query(&uri.to_query()).unwrap();
        assert_eq!(parsed, uri);
        assert_eq!(
            ParamsUri::from_bytes(&uri.to_bytes().unwrap()).unwrap(),
            uri
        );
        parsed
    };
    let v = compatible_e2e_version(&uri.vr).unwrap();
    let rcv = uri.to_rcv_params(v).unwrap();
    let use_kem = reply_kem(rcv.kem_key(), PqSupport(join_pq));
    let (b_priv, b_params) = generate_snd_params(v, use_kem.as_ref()).unwrap();
    let b_params = E2eParams::from_bytes(&b_params.to_bytes().unwrap()).unwrap();
    let bob_init = pq_x3dh_snd(&b_priv, &rcv).unwrap();
    let alice_init = pq_x3dh_rcv(&a_priv, &b_params).unwrap();
    assert_same_init(&alice_init.0, &bob_init.0);
    let versions = RatchetVersions::new(v);
    let dhrs = KeyPair::generate(Alg::X448).unwrap();
    let bob_dhrs = dhrs.public.clone();
    let dhrs = dhrs.private;
    let (bob_i, alice_i) = (bob_init.0.clone(), alice_init.0.clone());
    Pair {
        bob: Ratchet::init_snd(versions, rcv.params().k2.clone(), dhrs, bob_init).unwrap(),
        alice: Ratchet::init_rcv(
            versions,
            a_priv.pk2,
            alice_init,
            PqSupport(invite_pq && join_pq),
        ),
        alice_init: alice_i,
        bob_init: bob_i,
        bob_dhrs,
    }
}

pub const MODES: [(&str, bool, bool); 4] = [
    ("PQ off", false, false),
    ("PQ accepted", true, true),
    ("PQ proposed in reply", false, true),
    ("PQ proposed, declined", true, false),
];

pub fn send(r: &mut Ratchet, text: &str) -> Vec<u8> {
    r.encrypt(text.as_bytes(), PADDED, None).unwrap()
}

pub fn send_pq(r: &mut Ratchet, text: &str, pq: bool) -> Vec<u8> {
    r.encrypt(
        text.as_bytes(),
        PADDED,
        Some(zaun_chat::ratchet::PqEncryption(pq)),
    )
    .unwrap()
}

pub fn recv(r: &mut Ratchet, wire: &[u8]) -> String {
    String::from_utf8(r.decrypt(wire).unwrap()).unwrap()
}

pub fn recv_err(r: &mut Ratchet, wire: &[u8]) -> RatchetError {
    r.decrypt(wire).unwrap_err()
}

pub fn reload(r: &Ratchet) -> Ratchet {
    let bytes = r.to_bytes().unwrap();
    let back = Ratchet::from_bytes(&bytes).unwrap();
    assert_eq!(&back, r);
    back
}

/// Everything the two sides must share. The accepted KEM differs by design: each side keeps the other's key.
pub fn assert_same_init(a: &InitParams, b: &InitParams) {
    assert_eq!(a.assoc_data, b.assoc_data);
    assert_eq!(a.verify_code_pq, b.verify_code_pq);
    assert_eq!(a.ratchet_key, b.ratchet_key);
    assert_eq!((&a.snd_hk, &a.rcv_next_hk), (&b.snd_hk, &b.rcv_next_hk));
    assert_eq!(
        a.kem_accepted.as_ref().map(|k| (&k.shared, &k.ct)),
        b.kem_accepted.as_ref().map(|k| (&k.shared, &k.ct))
    );
}
