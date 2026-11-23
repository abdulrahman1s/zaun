//! Crypto tests: known answers from nook's own code, RFC vectors, and the rules of each primitive.
use poly1305::{Poly1305 as CratePoly, universal_hash::KeyInit};
use zaun_chat::crypto::*;
use zaun_chat::encoding::{Decode, Encode};

use crate::hex::{arr, hex, unhex};
use crate::vectors::*;

fn nonce() -> CbNonce {
    CbNonce::new(arr(NONCE24))
}

fn key() -> SbKey {
    SbKey::new(arr(KEY))
}

fn msg() -> Vec<u8> {
    unhex(MSG)
}

#[test]
fn ed25519_matches_rfc8032_vector_1_and_nook_signature() {
    let sk = PrivateKey::from_raw(
        Alg::Ed25519,
        &unhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"),
    )
    .unwrap();
    let pk = sk.public().unwrap();
    assert_eq!(
        hex(pk.as_bytes()),
        "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"
    );
    let sig = sk.sign(b"").unwrap();
    assert_eq!(
        hex(sig.as_bytes()),
        "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
    );
    assert!(pk.verify(&sig, b""));
    let sk = PrivateKey::from_raw(Alg::Ed25519, &unhex(ED25519_SEED)).unwrap();
    assert_eq!(hex(sk.public().unwrap().as_bytes()), ED25519_PUB);
    let m = &msg()[..40];
    let sig = sk.sign(m).unwrap();
    assert_eq!(hex(sig.as_bytes()), ED25519_SIG);
    assert!(!sk.public().unwrap().verify(&sig, &msg()[..41]));
}

#[test]
fn ed448_signature_equals_nook_and_openssl_and_rejects_tamper() {
    let sk = PrivateKey::from_raw(Alg::Ed448, &unhex(ED448_SEED)).unwrap();
    let pk = sk.public().unwrap();
    assert_eq!(hex(pk.as_bytes()), ED448_PUB);
    let m = unhex(ED448_MSG);
    let sig = sk.sign(&m).unwrap();
    assert_eq!(hex(sig.as_bytes()), ED448_SIG);
    assert_eq!(sig.as_bytes().len(), 114);
    assert!(pk.verify(&sig, &m));
    assert!(!pk.verify(&sig, &m[..39]));
    assert!(pk.verify(
        &Signature::from_raw(Alg::Ed448, &unhex(ED448_SIG)).unwrap(),
        &m
    ));
}

#[test]
fn ed448_matches_rfc8032_blank_message_vector() {
    let sk = PrivateKey::from_raw(Alg::Ed448, &unhex("6c82a562cb808d10d632be89c8513ebf6c929f34ddfa8c9f63c9960ef6e348a3528c8a3fcc2f044e39a3fc5b94492f8f032e7549a20098f95b")).unwrap();
    assert_eq!(
        hex(sk.public().unwrap().as_bytes()),
        "5fd7449b59b461fd2ce787ec616ad46a1da1342485a70e1f8a0ea75d80e96778edf124769b46c7061bd6783df1e50f6cd1fa1abeafe8256180"
    );
    assert_eq!(
        hex(sk.sign(b"").unwrap().as_bytes()),
        "533a37f6bbe457251f023c0d88f976ae2dfb504a843e34d2074fd823d41a591f2b233f034f628281f2fd7a22ddd47d7828c59bd0a21bfd3980ff0d2028d4b18a9df63e006c5d1c2d345b925d8dc00b4104852db99ac5c7cdda8530a113a0f4dbb61149f05a7363268c71d95808ff2e652600"
    );
}

#[test]
fn signature_of_the_wrong_algorithm_does_not_verify() {
    let ed = KeyPair::generate(Alg::Ed25519).unwrap();
    let ed4 = KeyPair::generate(Alg::Ed448).unwrap();
    let sig = ed4.private.sign(b"m").unwrap();
    assert!(!ed.public.verify(&sig, b"m"));
    assert!(
        KeyPair::generate(Alg::X25519)
            .unwrap()
            .private
            .sign(b"m")
            .is_err()
    );
}

#[test]
fn x25519_matches_rfc7748_section_6_1() {
    let a = PrivateKey::from_raw(
        Alg::X25519,
        &unhex("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a"),
    )
    .unwrap();
    let b = PrivateKey::from_raw(
        Alg::X25519,
        &unhex("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb"),
    )
    .unwrap();
    let (pa, pb) = (a.public().unwrap(), b.public().unwrap());
    assert_eq!(
        hex(pa.as_bytes()),
        "8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"
    );
    assert_eq!(
        hex(pb.as_bytes()),
        "de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"
    );
    let k = "4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742";
    assert_eq!(hex(dh(&pb, &a).unwrap().as_bytes()), k);
    assert_eq!(hex(dh(&pa, &b).unwrap().as_bytes()), k);
}

#[test]
fn x448_matches_nook_and_rfc7748_section_6_2() {
    let a = PrivateKey::from_raw(Alg::X448, &unhex(X448_A_PRIV)).unwrap();
    let b = PrivateKey::from_raw(Alg::X448, &unhex(X448_B_PRIV)).unwrap();
    let (pa, pb) = (a.public().unwrap(), b.public().unwrap());
    assert_eq!(hex(pa.as_bytes()), X448_A_PUB);
    assert_eq!(hex(pb.as_bytes()), X448_B_PUB);
    assert_eq!(hex(dh(&pa, &b).unwrap().as_bytes()), X448_SHARED);
    assert_eq!(
        dh(&pb, &a).unwrap().as_bytes(),
        dh(&pa, &b).unwrap().as_bytes()
    );

    let a = PrivateKey::from_raw(Alg::X448, &unhex("9a8f4925d1519f5775cf46b04b5800d4ee9ee8bae8bc5565d498c28dd9c9baf574a9419744897391006382a6f127ab1d9ac2d8c0a598726b")).unwrap();
    let b = PrivateKey::from_raw(Alg::X448, &unhex("1c306a7ac2a0e2e0990b294470cba339e6453772b075811d8fad0d1d6927c120bb5ee8972b0d3e21374c9c921b09d1b0366f10b65173992d")).unwrap();
    assert_eq!(
        hex(a.public().unwrap().as_bytes()),
        "9b08f7cc31b7e3e67d22d5aea121074a273bd2b83de09c63faa73d2c22c5d9bbc836647241d953d40c5b12da88120d53177f80e532c41fa0"
    );
    assert_eq!(
        hex(dh(&b.public().unwrap(), &a).unwrap().as_bytes()),
        "07fff4181ac6cc95ec1c16a94a0f74d12da232ce40a77552281d282bb60c0b56fd2464c335543936521c24403085d59a449a5037514a879d"
    );
}

#[test]
fn dh_of_mismatched_algorithms_fails() {
    let a = KeyPair::generate(Alg::X25519).unwrap();
    let b = KeyPair::generate(Alg::X448).unwrap();
    assert!(dh(&a.public, &b.private).is_err());
    assert!(dh(&KeyPair::generate(Alg::Ed25519).unwrap().public, &a.private).is_err());
}

#[test]
fn der_prefixes_and_sizes_match_nook() {
    let ed = PrivateKey::from_raw(
        Alg::Ed25519,
        &unhex("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"),
    )
    .unwrap();
    assert_eq!(
        hex(&ed.public().unwrap().to_der()[..12]),
        "302a300506032b6570032100"
    );
    assert_eq!(hex(&ed.to_der()[..16]), "302e020100300506032b657004220420");
    for (alg, pub_len, priv_len) in [
        (Alg::Ed25519, 44, 48),
        (Alg::X25519, 44, 48),
        (Alg::Ed448, 69, 73),
        (Alg::X448, 68, 72),
    ] {
        let kp = KeyPair::generate(alg).unwrap();
        let (pub_der, priv_der) = (kp.public.to_der(), kp.private.to_der());
        assert_eq!(
            (pub_der.len(), priv_der.len()),
            (pub_len, priv_len),
            "{alg:?}"
        );
        assert_eq!(PublicKey::from_der(&pub_der).unwrap(), kp.public);
        assert_eq!(PrivateKey::from_der(&priv_der).unwrap(), kp.private);
    }
}

#[test]
fn der_decoding_rejects_wrong_sizes_oids_and_trailing_bytes() {
    assert!(PublicKey::from_der(&[0u8; 44]).is_err());
    assert!(PublicKey::from_der(&[]).is_err());
    let mut der = KeyPair::generate(Alg::Ed25519).unwrap().public.to_der();
    der.push(0);
    assert!(PublicKey::from_der(&der).is_err());
    der.pop();
    der[8] = 0x72;
    assert!(PublicKey::from_der(&der).is_err());
    assert!(PrivateKey::from_der(&[0u8; 48]).is_err());
}

#[test]
fn keys_encode_as_a_bytestring_of_their_der() {
    let kp = KeyPair::generate(Alg::X25519).unwrap();
    let bytes = kp.public.to_bytes().unwrap();
    assert_eq!(bytes[0], 44);
    assert_eq!(&bytes[1..], kp.public.to_der());
    assert_eq!(PublicKey::from_bytes(&bytes).unwrap(), kp.public);
    assert_eq!(
        PrivateKey::from_bytes(&kp.private.to_bytes().unwrap()).unwrap(),
        kp.private
    );
    let sig = KeyPair::generate(Alg::Ed448)
        .unwrap()
        .private
        .sign(b"x")
        .unwrap();
    assert_eq!(
        Signature::from_bytes(&sig.to_bytes().unwrap()).unwrap(),
        sig
    );
}

#[test]
fn private_key_debug_does_not_print_the_secret() {
    let kp = KeyPair::generate(Alg::Ed25519).unwrap();
    assert_eq!(format!("{:?}", kp.private), "PrivateKey::Ed25519(..)");
    assert_eq!(format!("{:?}", SbKey::random().unwrap()), "SbKey(..)");
}

#[test]
fn sha_family_matches_fips_and_nook() {
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(&sha512(b"abc")),
        "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
    );
    assert_eq!(
        hex(&sha3_256(b"abc")),
        "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
    );
    assert_eq!(
        hex(&sha3_384(b"abc")),
        "ec01498288516fc926459f58e2c6ad8df9b473cb0fc08c2596da7cf0e49be4b298d88cea927ac7f539f1edf228376d25"
    );
    let m = msg();
    assert_eq!(hex(&sha256(&m)), SHA256);
    assert_eq!(hex(&sha512(&m)), SHA512);
    assert_eq!(hex(&sha3_256(&m)), SHA3_256);
    assert_eq!(hex(&sha3_384(&m)), SHA3_384);
    assert_eq!(key_hash(&m), sha256(&m));
}

fn seq(n: usize, k: usize) -> Vec<u8> {
    (0..n).map(|i| ((i * 7 + k) & 255) as u8).collect()
}

#[test]
fn hkdf_sha512_equals_nook() {
    let out = hkdf_sha512::<88>(&seq(32, 2), &seq(32, 4), b"SimpleXSbChain").unwrap();
    assert_eq!(hex(&*out), HKDF);
}

#[test]
fn sb_chain_derivation_equals_nook() {
    let (c1, c2) = sbc_init(&seq(32, 5), &seq(32, 6)).unwrap();
    assert_eq!(
        format!("{}{}", hex(c1.as_bytes()), hex(c2.as_bytes())),
        SBC_INIT
    );
    let (key, nonce, next) = sbc_hkdf(&c1).unwrap();
    assert_eq!(
        format!(
            "{}{}{}",
            hex(next.as_bytes()),
            hex(key.as_bytes()),
            hex(nonce.as_bytes())
        ),
        SBC_HKDF
    );
}

#[test]
fn aes256_gcm_with_16_byte_iv_equals_nook_and_openssl() {
    let k = AesKey::new(arr(KEY));
    let (iv, ad) = (arr::<16>(GCM_IV), unhex(GCM_AD));
    let m = &msg()[..100];
    let (ct, tag) = aead_encrypt(&k, &iv, 200, &ad, m).unwrap();
    assert_eq!(hex(&ct), GCM_CT);
    assert_eq!(hex(&tag), GCM_TAG);
    assert_eq!(aead_decrypt(&k, &iv, &ad, &ct, &tag).unwrap(), m);
}

#[test]
fn aes_gcm_rejects_tampered_ciphertext_tag_and_associated_data() {
    let k = AesKey::new(arr(KEY));
    let (iv, ad) = (arr::<16>(GCM_IV), unhex(GCM_AD));
    let (ct, tag) = aead_encrypt(&k, &iv, 200, &ad, b"hello").unwrap();
    let mut bad = ct.clone();
    bad[3] ^= 1;
    assert!(aead_decrypt(&k, &iv, &ad, &bad, &tag).is_err());
    let mut bad_tag = tag;
    bad_tag[0] ^= 1;
    assert!(aead_decrypt(&k, &iv, &ad, &ct, &bad_tag).is_err());
    assert!(aead_decrypt(&k, &iv, b"other", &ct, &tag).is_err());
    assert!(aead_encrypt(&k, &iv, 4, &ad, b"hello").is_err());
}

#[test]
fn xchacha20_poly1305_equals_noble_and_binds_the_aad() {
    let k: [u8; 32] = arr(KEY);
    let (n, ad) = (arr::<24>(XC_NONCE), unhex(XC_AD));
    let sealed = xchacha_seal_with_nonce(&k, &n, &ad, &msg()[..100]).unwrap();
    assert_eq!(hex(&sealed), format!("{XC_NONCE}{XC_CT}"));
    assert_eq!(xchacha_open(&k, &ad, &sealed).unwrap(), &msg()[..100]);
    assert!(xchacha_open(&k, b"other", &sealed).is_err());
    assert!(xchacha_open(&k, &ad, &sealed[..20]).is_err());
}

#[test]
fn xchacha_seal_uses_a_fresh_nonce_each_time() {
    let k = [3u8; 32];
    assert_ne!(
        xchacha_seal(&k, b"", b"x").unwrap(),
        xchacha_seal(&k, b"", b"x").unwrap()
    );
}

#[test]
fn hsalsa20_box_key_and_crypto_box_equal_nook() {
    assert_eq!(hex(&*box_key(&arr(KEY))), BOX_KEY);
    let m = &msg()[..100];
    let ct = crypto_box(&arr(KEY), &nonce(), m);
    assert_eq!(hex(&ct), BOX_CT);
    assert_eq!(crypto_box_open(&arr(KEY), &nonce(), &ct).unwrap(), m);
}

#[test]
fn crypto_box_rejects_a_flipped_byte_and_a_short_packet() {
    let mut ct = crypto_box(&arr(KEY), &nonce(), b"secret");
    assert!(crypto_box_open(&arr(KEY), &nonce(), &ct[..15]).is_err());
    ct[17] ^= 1;
    assert_eq!(
        crypto_box_open(&arr(KEY), &nonce(), &ct),
        Err(CryptoError::CbDecrypt)
    );
}

#[test]
fn sb_encrypt_pads_and_round_trips() {
    let ct = sb_encrypt(&key(), &nonce(), b"abc", 100).unwrap();
    assert_eq!(ct.len(), 116);
    assert_eq!(sb_decrypt(&key(), &nonce(), &ct).unwrap(), b"abc");
    assert_eq!(
        sb_decrypt_no_pad(
            &key(),
            &nonce(),
            &sb_encrypt_no_pad(&key(), &nonce(), b"abc")
        )
        .unwrap(),
        b"abc"
    );
    assert_eq!(
        sb_encrypt(&key(), &nonce(), &[0; 99], 100),
        Err(CryptoError::LargeMsg)
    );
}

fn x25519_pair() -> (PublicKey, PrivateKey) {
    let srv = PrivateKey::from_raw(Alg::X25519, &unhex(X_CLI_PRIV)).unwrap();
    (srv.public().unwrap(), srv)
}

#[test]
fn cb_authenticate_equals_nook_and_is_80_bytes() {
    let srv = PublicKey::from_raw(Alg::X25519, &unhex(X_SRV_PUB)).unwrap();
    let cli = PrivateKey::from_raw(Alg::X25519, &unhex(X_CLI_PRIV)).unwrap();
    assert_eq!(hex(dh(&srv, &cli).unwrap().as_bytes()), DH);
    let m = unhex(CB_AUTH_MSG);
    let auth = cb_authenticate(&srv, &cli, &nonce(), &m).unwrap();
    assert_eq!(auth.len(), CB_AUTHENTICATOR_LEN);
    assert_eq!(hex(&auth), CB_AUTH);
}

#[test]
fn cb_verify_accepts_the_other_side_and_rejects_another_message() {
    let (cli_pub, cli_priv) = x25519_pair();
    let srv = KeyPair::generate(Alg::X25519).unwrap();
    let auth = cb_authenticate(&srv.public, &cli_priv, &nonce(), b"msg").unwrap();
    assert!(cb_verify(&cli_pub, &srv.private, &nonce(), &auth, b"msg"));
    assert!(!cb_verify(
        &cli_pub,
        &srv.private,
        &nonce(),
        &auth,
        b"other"
    ));
    assert!(!cb_verify(
        &cli_pub,
        &srv.private,
        &nonce(),
        &auth[..79],
        b"msg"
    ));
}

#[test]
fn cb_encrypt_round_trips_between_the_two_sides_and_detects_tampering() {
    let (a, b) = (
        KeyPair::generate(Alg::X25519).unwrap(),
        KeyPair::generate(Alg::X25519).unwrap(),
    );
    let (s1, s2) = (
        dh(&a.public, &b.private).unwrap(),
        dh(&b.public, &a.private).unwrap(),
    );
    let n = CbNonce::random().unwrap();
    let mut c = cb_encrypt(&s1, &n, b"hello", 200).unwrap();
    assert_eq!(c.len(), 216);
    assert_eq!(cb_decrypt(&s2, &n, &c).unwrap(), b"hello");
    c[40] ^= 1;
    assert_eq!(cb_decrypt(&s2, &n, &c), Err(CryptoError::CbDecrypt));
}

#[test]
fn cb_decrypt_rejects_an_all_zero_secret() {
    let zero = DhSecret::X25519(zeroize::Zeroizing::new([0u8; 32]));
    let c = cb_encrypt_no_pad(&zero, &nonce(), b"x").unwrap();
    assert_eq!(cb_decrypt(&zero, &nonce(), &c), Err(CryptoError::CbDecrypt));
}

#[test]
fn boxes_only_take_an_x25519_secret() {
    let k = KeyPair::generate(Alg::X448).unwrap();
    let s = dh(&k.public, &k.private).unwrap();
    assert!(cb_encrypt(&s, &nonce(), b"x", 10).is_err());
}

#[test]
fn streaming_secretbox_in_any_segments_equals_one_shot_and_nook() {
    let m = msg();
    let mut sb = SbState::new(&key(), &nonce());
    let mut out = Vec::new();
    for r in [0..7, 7..300, 300..1000] {
        let mut p = m[r].to_vec();
        sb.encrypt(&mut p);
        out.extend(p);
    }
    assert_eq!(hex(&out), SB_CIPHER);
    assert_eq!(hex(&sb.tag()), SB_TAG);
    let one = crypto_box(key().as_bytes(), &nonce(), &m);
    assert_eq!(&one[16..], &out[..]);
    assert_eq!(hex(&one[..16]), SB_TAG);
}

#[test]
fn streaming_secretbox_decrypts_in_odd_segments_to_the_same_tag() {
    let ct = unhex(SB_CIPHER);
    let mut sb = SbState::new(&key(), &nonce());
    let mut dec = ct.clone();
    for r in [0..13, 13..16, 16..17, 17..1000] {
        sb.decrypt(&mut dec[r]);
    }
    assert_eq!(dec, msg());
    assert_eq!(hex(&sb.tag()), SB_TAG);
}

#[test]
fn tail_tag_secretbox_puts_the_tag_last_and_round_trips() {
    let sealed = sb_seal_tail_tag(&key(), &nonce(), b"file bytes", 64).unwrap();
    assert_eq!(sealed.len(), 64 + 16);
    assert_eq!(
        sb_open_tail_tag(&key(), &nonce(), 64, &sealed).unwrap(),
        b"file bytes"
    );
    let plain = sb_seal_tail_tag_no_pad(&key(), &nonce(), &msg());
    assert_eq!(hex(&plain[..1000]), SB_CIPHER);
    assert_eq!(hex(&plain[1000..]), SB_TAG);
    assert_eq!(
        sb_open_tail_tag_no_pad(&key(), &nonce(), &plain).unwrap(),
        msg()
    );
}

#[test]
fn tail_tag_secretbox_rejects_a_bad_tag_a_flipped_byte_and_a_wrong_length() {
    let mut sealed = sb_seal_tail_tag(&key(), &nonce(), b"file bytes", 64).unwrap();
    assert!(sb_open_tail_tag(&key(), &nonce(), 63, &sealed).is_err());
    sealed[5] ^= 1;
    assert!(sb_open_tail_tag(&key(), &nonce(), 64, &sealed).is_err());
    assert!(sb_open_tail_tag_no_pad(&key(), &nonce(), &[0; 15]).is_err());
}

#[test]
fn streaming_poly1305_matches_the_crate_for_all_lengths_and_splits() {
    let key: [u8; 32] = core::array::from_fn(|i| (i * 13 + 5) as u8);
    let data: Vec<u8> = (0..200u32).map(|i| (i * 31 + 7) as u8).collect();
    for len in 0..=200 {
        let want = CratePoly::new(&key.into()).compute_unpadded(&data[..len]);
        for split in [0, 1, 15, 16, 17, 33] {
            let mut p = Poly1305::new(&key);
            let s = split.min(len);
            p.update(&data[..s]);
            p.update(&data[s..len]);
            assert_eq!(p.finalize()[..], want[..], "len {len} split {split}");
        }
    }
}

#[test]
fn poly1305_matches_rfc8439_section_2_5_2() {
    let mut p = Poly1305::new(&arr(
        "85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b",
    ));
    p.update(b"Cryptographic Forum Research Group");
    assert_eq!(hex(&p.finalize()), "a8061dc1305136c6c22b8baf0c0127a9");
}

#[test]
fn pad_layout_is_two_byte_length_then_hashes() {
    assert_eq!(hex(&pad(&[1, 2, 3], 10).unwrap()), PAD);
    assert_eq!(hex(&pad(&[1, 2, 3], 5).unwrap()), "0003010203");
    assert_eq!(unpad(&pad(&[1, 2, 3], 10).unwrap()).unwrap(), [1, 2, 3]);
}

#[test]
fn pad_rejects_messages_that_do_not_fit_and_unpad_rejects_short_input() {
    assert_eq!(pad(&[0; 9], 10), Err(CryptoError::LargeMsg));
    assert_eq!(
        pad(&vec![0; MAX_MSG_LEN + 1], 70000),
        Err(CryptoError::LargeMsg)
    );
    assert!(can_pad(MAX_MSG_LEN, MAX_MSG_LEN + 2));
    assert_eq!(unpad(&[0, 9, 1]), Err(CryptoError::InvalidMsg));
    assert_eq!(unpad(&[0]), Err(CryptoError::InvalidMsg));
}

#[test]
fn lazy_pad_uses_an_eight_byte_length_and_split_len_tolerates_a_short_tail() {
    let p = pad_lazy(b"abcdef", 3, 16).unwrap();
    assert_eq!(hex(&p), "0000000000000003616263".to_owned() + "2323232323");
    assert_eq!(unpad_lazy(&p).unwrap(), b"abc");
    assert_eq!(split_len(&p[..9]).unwrap(), (3, &b"a"[..]));
    assert!(pad_lazy(b"abc", 3, 10).is_err());
    assert!(unpad_lazy(&[0; 7]).is_err());
    assert!(unpad_lazy(&[0xff; 8]).is_err());
}

#[test]
fn xor_nonce_xors_only_the_prefix() {
    let n = CbNonce::new([0xff; 24]);
    let x = xor_nonce(&[0x0f, 0xf0], &n);
    assert_eq!(&x.as_bytes()[..3], [0xf0, 0x0f, 0xff]);
    assert_eq!(xor_nonce(&[], &n), n);
}

#[test]
fn random_bytes_differ_between_calls_and_have_the_requested_size() {
    assert_eq!(random_vec(33).unwrap().len(), 33);
    assert_ne!(random_array::<32>().unwrap(), random_array::<32>().unwrap());
}
