//! Certificate parsing: keys come out of canonical certificates, malformed input is refused.
use crate::crypto::{Alg, KeyPair};
use crate::tls::certificate_public_key;
use crate::tls::testing::make_cert;

#[test]
fn leaf_key_of_an_ed25519_certificate_is_read() {
    let k = KeyPair::generate(Alg::Ed25519).unwrap();
    let cert = make_cert(&k.public, &k, 1);
    assert_eq!(certificate_public_key(&cert).unwrap(), k.public);
}

#[test]
fn leaf_key_of_an_ed448_certificate_is_read() {
    let k = KeyPair::generate(Alg::Ed448).unwrap();
    let cert = make_cert(&k.public, &k, 1);
    assert_eq!(certificate_public_key(&cert).unwrap(), k.public);
}

#[test]
fn truncated_certificate_is_refused() {
    let k = KeyPair::generate(Alg::Ed25519).unwrap();
    let cert = make_cert(&k.public, &k, 1);
    for cut in [0, 1, 2, 10, cert.len() - 1] {
        assert!(
            certificate_public_key(&cert[..cut]).is_err(),
            "cut at {cut}"
        );
    }
}

#[test]
fn trailing_bytes_after_the_certificate_are_refused() {
    let k = KeyPair::generate(Alg::Ed25519).unwrap();
    let mut cert = make_cert(&k.public, &k, 1);
    cert.push(0);
    assert!(certificate_public_key(&cert).is_err());
}

#[test]
fn unsupported_key_type_is_refused() {
    let k = KeyPair::generate(Alg::Ed25519).unwrap();
    let mut cert = make_cert(&k.public, &k, 1);
    // the OID occurs in the tbs signature algorithm, then in the key
    let at = (0..cert.len() - 5)
        .filter(|i| cert[*i..*i + 5] == [0x06, 0x03, 0x2b, 0x65, 0x70])
        .nth(1)
        .unwrap();
    cert[at + 4] = 0x72; // an OID the key decoder does not know
    assert!(certificate_public_key(&cert).is_err());
}

#[test]
fn x25519_certificate_key_is_read_as_a_dh_key() {
    let signer = KeyPair::generate(Alg::Ed25519).unwrap();
    let dh = KeyPair::generate(Alg::X25519).unwrap();
    let cert = make_cert(&dh.public, &signer, 2);
    assert_eq!(certificate_public_key(&cert).unwrap().alg(), Alg::X25519);
}
