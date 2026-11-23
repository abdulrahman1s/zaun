//! Handshake params: binary and link encodings.
use zaun_chat::crypto::{Alg, KeyPair};
use zaun_chat::encoding::{Decode, Encode, Version, VersionRange};
use zaun_chat::ratchet::{
    CURRENT_E2E_VERSION, E2eParams, KemParams, ParamsUri, PqSupport, PrivParams, RcvParams,
    adjust_e2e_range, compatible_e2e_version, generate_rcv_params, generate_snd_params, reply_kem,
    supported_e2e_range,
};

#[test]
fn params_round_trip_in_binary_and_link_form_with_and_without_pq() {
    for pq in [false, true] {
        let (_, p) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport(pq)).unwrap();
        assert_eq!(RcvParams::from_bytes(&p.to_bytes().unwrap()).unwrap(), p);
        assert_eq!(p.kem_key().is_some(), pq);
        let uri = ParamsUri::from_params(&p, supported_e2e_range());
        assert_eq!(ParamsUri::from_query(&uri.to_query()).unwrap(), uri);
        assert_eq!(
            ParamsUri::from_bytes(&uri.to_bytes().unwrap()).unwrap(),
            uri
        );
        assert_eq!(uri.to_query().contains("kem_key="), pq);
        assert!(uri.to_query().starts_with("v=3&x3dh="));
    }
}

#[test]
fn accepted_kem_params_write_the_ciphertext_before_the_key() {
    let (_, rcv) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport::ON).unwrap();
    let use_kem = reply_kem(rcv.kem_key(), PqSupport::ON);
    let (_, snd) = generate_snd_params(CURRENT_E2E_VERSION, use_kem.as_ref()).unwrap();
    assert!(matches!(snd.kem, Some(KemParams::Accepted { .. })));
    let uri = ParamsUri {
        vr: supported_e2e_range(),
        k1: snd.k1.clone(),
        k2: snd.k2.clone(),
        kem: snd.kem.clone(),
    };
    let q = uri.to_query();
    assert!(q.find("&kem_ct=").unwrap() < q.find("&kem_key=").unwrap());
    assert_eq!(ParamsUri::from_query(&q).unwrap(), uri);
    // a link may carry only proposals
    assert!(uri.to_rcv_params(CURRENT_E2E_VERSION).is_err());
    assert!(RcvParams::from_bytes(&snd.to_bytes().unwrap()).is_err());
    assert_eq!(
        E2eParams::from_bytes(&snd.to_bytes().unwrap()).unwrap(),
        snd
    );
}

#[test]
fn version_ranges_are_adjusted_to_the_supported_minimum() {
    let r = |a, b| VersionRange::new(Version(a), Version(b)).unwrap();
    assert_eq!(adjust_e2e_range(r(1, 3)), r(3, 3));
    assert_eq!(adjust_e2e_range(r(1, 2)), r(3, 3));
    assert_eq!(adjust_e2e_range(r(3, 5)), r(3, 5));
    assert_eq!(compatible_e2e_version(&r(3, 5)), Some(Version(3)));
    assert_eq!(compatible_e2e_version(&r(4, 5)), None);
    assert_eq!(compatible_e2e_version(&r(1, 2)), None);
}

#[test]
fn link_params_without_x3dh_or_with_kem_ct_alone_are_rejected() {
    assert!(ParamsUri::from_query("v=3").is_err());
    let (_, p) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport::OFF).unwrap();
    let q = ParamsUri::from_params(&p, supported_e2e_range()).to_query();
    assert!(ParamsUri::from_query(&format!("{q}&kem_ct=AAAA")).is_err());
    assert!(ParamsUri::from_query(&q.replace("v=3", "v=x")).is_err());
}

#[test]
fn params_with_keys_of_another_algorithm_are_rejected() {
    let x25 = KeyPair::generate(Alg::X25519).unwrap().public;
    let x448 = KeyPair::generate(Alg::X448).unwrap().public;
    assert!(E2eParams::new(CURRENT_E2E_VERSION, x25.clone(), x448.clone(), None).is_err());
    let bytes = (CURRENT_E2E_VERSION, &x25, &x448, None::<KemParams>)
        .to_bytes()
        .unwrap();
    assert!(E2eParams::from_bytes(&bytes).is_err());
}

#[test]
fn private_params_survive_storage_between_invitation_and_reply() {
    for pq in [false, true] {
        let (private, public) = generate_rcv_params(CURRENT_E2E_VERSION, PqSupport(pq)).unwrap();
        let back = PrivParams::from_bytes(&private.to_bytes().unwrap()).unwrap();
        assert_eq!(back, private);
        assert_eq!(back.rcv_params(CURRENT_E2E_VERSION).unwrap(), public);
    }
}
