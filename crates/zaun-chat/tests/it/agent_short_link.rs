//! Short link data: the key derivation, signing, encryption and checks of Crypto/ShortLink.hs, ported from simplexmq's ShortLinkTests.hs and nook's links.test.ts.
use zaun_chat::agent::AgentError;
use zaun_chat::agent::links::ConnectionMode;
use zaun_chat::agent::short_link::{
    ContactShortLinkData, EncLinkData, OwnerAuth, UserConnLinkData, UserContactData, UserLinkData,
    contact_short_link_kdf, decode_short_link_data, decrypt_link_data, encode_short_link_data,
    encode_sign_fixed_data, encode_sign_user_data, encrypt_fixed_data, encrypt_user_data,
    inv_short_link_kdf,
};
use zaun_chat::agent::version::supported_agent_range;
use zaun_chat::chat::profile::Profile;
use zaun_chat::crypto::{Alg, KeyPair, PrivateKey, SbKey, random_vec};
use zaun_chat::encoding::b64_encode;

use super::agent_support::*;
use super::hex::{hex, unhex};

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

fn root() -> KeyPair {
    KeyPair::generate(Alg::Ed25519).unwrap()
}

fn user(data: &[u8]) -> UserContactData {
    UserContactData {
        direct: true,
        owners: vec![],
        relays: vec![],
        user_data: UserLinkData(data.to_vec()),
        ratchet_keys: None,
    }
}

struct Contact {
    link_key: Vec<u8>,
    key: SbKey,
    fixed: Vec<u8>,
}

fn contact_link(root: &PrivateKey, data: UserContactData) -> (Contact, Vec<u8>) {
    let req = inv(CLI_CONTACT_FULL);
    let (link_key, signed) =
        encode_sign_fixed_data(root, supported_agent_range(), &req, None).unwrap();
    let (_, key) = contact_short_link_kdf(&link_key).unwrap();
    let fixed = encrypt_fixed_data(&key, &signed).unwrap();
    let signed_user = encode_sign_user_data(
        root,
        supported_agent_range(),
        UserConnLinkData::Contact(data),
    )
    .unwrap();
    let user = encrypt_user_data(&key, &signed_user).unwrap();
    (
        Contact {
            link_key,
            key,
            fixed,
        },
        user,
    )
}

fn open(c: &Contact, user: Vec<u8>) -> Result<UserContactData, AgentError> {
    let enc = EncLinkData {
        fixed: c.fixed.clone(),
        user,
    };
    let (fixed, link) = decrypt_link_data(ConnectionMode::Contact, &c.link_key, &c.key, &enc)?;
    assert_eq!(fixed.link_conn_req, inv(CLI_CONTACT_FULL).binary());
    match link {
        zaun_chat::agent::short_link::ConnLinkData::Contact { data, .. } => Ok(data),
        zaun_chat::agent::short_link::ConnLinkData::Invitation { .. } => panic!("invitation data"),
    }
}

fn reseal(c: &Contact, signer: &PrivateKey, data: UserContactData) -> Vec<u8> {
    let signed = encode_sign_user_data(
        signer,
        supported_agent_range(),
        UserConnLinkData::Contact(data),
    )
    .unwrap();
    encrypt_user_data(&c.key, &signed).unwrap()
}

fn link_err(s: &str) -> AgentError {
    AgentError::Link(s.to_owned())
}

#[test]
fn kdf_matches_an_independent_hkdf_sha512() {
    let (id, key) = contact_short_link_kdf(KEY).unwrap();
    assert_eq!(hex(&id), "7bb6cac6ddda05661de73a9974926c4d3d30f3dd36e0e365");
    assert_eq!(
        key.as_bytes().as_slice(),
        unhex("92f91f61f480698b6edcbc6277359f463f5810065e47253f72657bb8847e0ebe")
    );
    let inv = inv_short_link_kdf(KEY).unwrap();
    assert_eq!(
        hex(inv.as_bytes()),
        "7414e908a26d11b2e23016afa771e45d62bcdd72a14237958069e33f9c2c2319"
    );
}

#[test]
fn invitation_link_data_encrypts_and_decrypts() {
    let root = root();
    let req = inv(CLI_INVITATION_FULL);
    let (link_key, fixed) = encode_sign_fixed_data(
        &root.private,
        supported_agent_range(),
        &req,
        Some(b"entity".to_vec()),
    )
    .unwrap();
    let key = inv_short_link_kdf(&link_key).unwrap();
    let data = UserConnLinkData::Invitation(UserLinkData(b"some user data".to_vec()));
    let signed = encode_sign_user_data(&root.private, supported_agent_range(), data).unwrap();
    let enc = EncLinkData {
        fixed: encrypt_fixed_data(&key, &fixed).unwrap(),
        user: encrypt_user_data(&key, &signed).unwrap(),
    };
    assert_eq!((enc.fixed.len(), enc.user.len()), (2048, 13824));
    let (f, l) = decrypt_link_data(ConnectionMode::Invitation, &link_key, &key, &enc).unwrap();
    assert_eq!(f.link_conn_req, req.binary());
    assert_eq!(f.link_entity_id.as_deref(), Some(&b"entity"[..]));
    assert_eq!(f.root_key, root.public);
    assert_eq!(l.user_data().0, b"some user data");
    let mut bad = enc.clone();
    bad.user[100] ^= 1;
    assert!(decrypt_link_data(ConnectionMode::Invitation, &link_key, &key, &bad).is_err());
    let other = random_vec(32).unwrap();
    let err = decrypt_link_data(ConnectionMode::Invitation, &other, &key, &enc).unwrap_err();
    assert_eq!(err, link_err("link data hash"));
    let err = decrypt_link_data(ConnectionMode::Contact, &link_key, &key, &enc).unwrap_err();
    assert_eq!(err, link_err("parse link data"));
}

#[test]
fn contact_link_data_encrypts_and_decrypts() {
    let root = root();
    let (c, user_enc) = contact_link(&root.private, user(b"some user data"));
    assert_eq!(open(&c, user_enc).unwrap(), user(b"some user data"));
}

#[test]
fn updated_user_data_is_accepted_under_the_same_link_key() {
    let root = root();
    let (c, _) = contact_link(&root.private, user(b"some user data"));
    let relay = |s: &str| zaun_chat::agent::links::ConnShortLink::parse(s).unwrap();
    let updated = UserContactData {
        direct: false,
        relays: vec![
            relay(
                "https://localhost/a#4AkRDmhf64tdRlN406g8lJRg5OCmhD6ynIhi6glOcCM?p=7001&c=LcJUMfVhwD8yxjAiSaDzzGF3-kLG4Uh0Fl_ZIjrRwjI",
            ),
            relay("https://localhost/a#4AkRDmhf64tdRlN406g8lJRg5OCmhD6ynIhi6glOcCM"),
        ],
        ..user(b"updated user data")
    };
    let enc = reseal(&c, &root.private, updated.clone());
    assert_eq!(open(&c, enc).unwrap(), updated);
}

#[test]
fn user_data_over_254_bytes_uses_the_large_form() {
    let root = root();
    let data = user(&[b'x'; 3000]);
    let (c, enc) = contact_link(&root.private, data.clone());
    assert_eq!(open(&c, enc).unwrap(), data);
}

#[test]
fn contact_data_with_other_keys_has_a_bad_hash_or_signature() {
    let root = root();
    let (c, _) = contact_link(&root.private, user(b"some user data"));
    let wrong = Contact {
        link_key: random_vec(32).unwrap(),
        key: SbKey::from_slice(c.key.as_bytes()).unwrap(),
        fixed: c.fixed.clone(),
    };
    let enc = reseal(&c, &root.private, user(b"x"));
    assert_eq!(open(&wrong, enc).unwrap_err(), link_err("link data hash"));
    let other = root_key_pair().private;
    let enc = reseal(&c, &other, user(b"updated user data"));
    assert_eq!(open(&c, enc).unwrap_err(), link_err("user data signature"));
}

fn root_key_pair() -> KeyPair {
    root()
}

fn new_owner(signer: &PrivateKey) -> (PrivateKey, OwnerAuth) {
    let kp = root();
    let owner_id = random_vec(16).unwrap();
    let signed = [owner_id.as_slice(), &kp.public.to_der()].concat();
    let auth = OwnerAuth {
        owner_id,
        owner_key: kp.public.clone(),
        auth_owner_sig: signer.sign(&signed).unwrap(),
    };
    (kp.private, auth)
}

fn with_owners(owners: Vec<OwnerAuth>) -> UserContactData {
    UserContactData {
        owners,
        ..user(b"updated user data")
    }
}

fn id_str(o: &OwnerAuth) -> String {
    b64_encode(&o.owner_id, false)
}

#[test]
fn an_authorised_owner_can_sign_user_data() {
    let root = root();
    let (c, _) = contact_link(&root.private, user(b"some user data"));
    let (owner_key, owner) = new_owner(&root.private);
    let data = with_owners(vec![owner]);
    for signer in [&root.private, &owner_key] {
        assert_eq!(open(&c, reseal(&c, signer, data.clone())).unwrap(), data);
    }
    let wrong = root_key_pair().private;
    assert_eq!(
        open(&c, reseal(&c, &wrong, data)).unwrap_err(),
        link_err("user data signature")
    );
}

#[test]
fn owners_authorised_in_a_chain_can_all_sign() {
    let root = root();
    let (c, _) = contact_link(&root.private, user(b"some user data"));
    let (pk1, o1) = new_owner(&root.private);
    let (_pk2, o2) = new_owner(&root.private);
    let (pk3, o3) = new_owner(&pk1);
    let (_pk4, o4) = new_owner(&pk1);
    let (pk5, o5) = new_owner(&pk3);
    let data = with_owners(vec![o1, o2, o3, o4, o5]);
    for signer in [&root.private, &pk1, &pk3, &pk5] {
        assert_eq!(open(&c, reseal(&c, signer, data.clone())).unwrap(), data);
    }
}

#[test]
fn invalid_owner_lists_are_rejected_with_the_reason() {
    let root = root();
    let (c, _) = contact_link(&root.private, user(b"some user data"));
    let fails = |signer: &PrivateKey, owners: Vec<OwnerAuth>, err: String| {
        assert_eq!(
            open(&c, reseal(&c, signer, with_owners(owners))).unwrap_err(),
            link_err(&err)
        );
    };
    let (owner_key, owner) = new_owner(&root.private);
    let as_root = OwnerAuth {
        owner_key: root.public.clone(),
        ..owner.clone()
    };
    let err = format!("owner key for ID {} matches root key", id_str(&owner));
    fails(&root.private, vec![as_root.clone()], err.clone());
    fails(&owner_key, vec![as_root], err);
    let (_, owner1) = new_owner(&root.private);
    let same_id = OwnerAuth {
        owner_id: owner.owner_id.clone(),
        ..owner1.clone()
    };
    fails(
        &root.private,
        vec![owner.clone(), same_id],
        format!("duplicate owner key or ID {}", id_str(&owner)),
    );
    let same_key = OwnerAuth {
        owner_key: owner.owner_key.clone(),
        ..owner1.clone()
    };
    fails(
        &root.private,
        vec![owner.clone(), same_key],
        format!("duplicate owner key or ID {}", id_str(&owner1)),
    );
    let (_, owner2) = new_owner(&owner_key);
    let good = with_owners(vec![owner.clone(), owner1.clone(), owner2.clone()]);
    assert_eq!(
        open(&c, reseal(&c, &root.private, good.clone())).unwrap(),
        good
    );
    let err = format!("invalid authorization of owner ID {}", id_str(&owner2));
    fails(
        &root.private,
        vec![owner2.clone(), owner.clone(), owner1.clone()],
        err,
    );
    let (_, stranger) = new_owner(&root_key_pair().private);
    let err = format!("invalid authorization of owner ID {}", id_str(&stranger));
    fails(&root.private, vec![stranger.clone()], err.clone());
    fails(&root.private, vec![owner, owner1, owner2, stranger], err);
}

fn short_data() -> ContactShortLinkData {
    let mut data = ContactShortLinkData::new(Profile::new("alice", "Alice"));
    data.business = false;
    data
}

#[test]
fn chat_user_data_is_json_with_profile_and_business() {
    let data = encode_short_link_data(&short_data()).unwrap();
    let text = std::str::from_utf8(&data.0).unwrap();
    assert!(
        text.starts_with(r#"{"profile":{"displayName":"alice","fullName":"Alice""#),
        "{text}"
    );
    assert!(text.ends_with(r#""business":false}"#), "{text}");
    assert!(!text.contains("message") && !text.contains("localBadge"));
    assert_eq!(
        decode_short_link_data::<ContactShortLinkData>(&data).unwrap(),
        Some(short_data())
    );
}

#[test]
fn chat_user_data_over_10240_bytes_is_compressed_behind_an_x() {
    let mut big = short_data();
    big.profile.description = Some("long text. ".repeat(1500));
    let data = encode_short_link_data(&big).unwrap();
    assert_eq!(data.0[0], b'X');
    assert!(data.0.len() < 10240);
    assert_eq!(
        decode_short_link_data::<ContactShortLinkData>(&data).unwrap(),
        Some(big)
    );
}

#[test]
fn empty_chat_user_data_is_no_data() {
    assert_eq!(
        decode_short_link_data::<ContactShortLinkData>(&UserLinkData::default()).unwrap(),
        None
    );
}
