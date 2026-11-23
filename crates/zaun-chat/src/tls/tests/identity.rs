//! The pinned identity: CA fingerprint and chain signatures for chains of two to four certificates.
use crate::crypto::{Alg, KeyPair, key_hash};
use crate::tls::testing::make_cert;
use crate::tls::{ChainCertificates, IdentityError, chain_id_ca_certs, verify_server_chain};

struct Pki {
    ca: KeyPair,
    chain: Vec<Vec<u8>>,
}

/// Leaf signed by the CA; the CA signed by the first of `extra` upper certificates (or by itself),
/// each upper one signed by the next, the last one self-signed.
fn pki(extra: usize, leaf_alg: Alg) -> Pki {
    let leaf = KeyPair::generate(leaf_alg).unwrap();
    let ca = KeyPair::generate(Alg::Ed448).unwrap();
    let uppers: Vec<KeyPair> = (0..extra)
        .map(|_| KeyPair::generate(Alg::Ed25519).unwrap())
        .collect();
    let mut chain = vec![make_cert(&leaf.public, &ca, 1)];
    chain.push(make_cert(&ca.public, uppers.first().unwrap_or(&ca), 2));
    for (i, upper) in uppers.iter().enumerate() {
        let issuer = uppers.get(i + 1).unwrap_or(upper);
        chain.push(make_cert(&upper.public, issuer, 3 + i as u8));
    }
    Pki { ca, chain }
}

fn ca_hash(p: &Pki) -> [u8; 32] {
    key_hash(&p.chain[1])
}

#[test]
fn chain_of_two_with_a_self_signed_ca_is_accepted() {
    let p = pki(0, Alg::Ed25519);
    verify_server_chain(&p.chain, &ca_hash(&p)).unwrap();
    assert!(p.ca.public.alg() == Alg::Ed448);
}

#[test]
fn chains_of_three_and_four_are_accepted_by_their_signatures() {
    for extra in [1, 2] {
        let p = pki(extra, Alg::Ed448);
        assert_eq!(p.chain.len(), 2 + extra);
        verify_server_chain(&p.chain, &ca_hash(&p)).unwrap();
    }
}

#[test]
fn wrong_ca_fingerprint_is_rejected() {
    let p = pki(0, Alg::Ed25519);
    assert_eq!(
        verify_server_chain(&p.chain, &[0u8; 32]),
        Err(IdentityError::Fingerprint)
    );
}

#[test]
fn chain_of_one_or_five_is_rejected_by_shape() {
    let p = pki(2, Alg::Ed25519);
    assert_eq!(
        verify_server_chain(&p.chain[..1], &ca_hash(&p)),
        Err(IdentityError::ChainShape(1))
    );
    let mut five = p.chain.clone();
    five.push(p.chain[3].clone());
    assert_eq!(
        verify_server_chain(&five, &ca_hash(&p)),
        Err(IdentityError::ChainShape(5))
    );
}

#[test]
fn leaf_not_signed_by_the_ca_is_rejected() {
    let mut p = pki(0, Alg::Ed25519);
    let other = KeyPair::generate(Alg::Ed25519).unwrap();
    p.chain[0] = make_cert(&other.public, &other, 9);
    assert_eq!(
        verify_server_chain(&p.chain, &ca_hash(&p)),
        Err(IdentityError::Signature(0))
    );
}

#[test]
fn ca_of_a_two_chain_that_does_not_sign_itself_is_rejected() {
    let mut p = pki(0, Alg::Ed25519);
    let other = KeyPair::generate(Alg::Ed25519).unwrap();
    p.chain[1] = make_cert(&p.ca.public, &other, 9);
    let h = ca_hash(&p);
    assert_eq!(
        verify_server_chain(&p.chain, &h),
        Err(IdentityError::NotSelfSigned)
    );
}

#[test]
fn chain_roles_follow_simplexmq_chain_id_ca_certs() {
    let c: Vec<Vec<u8>> = (0..5u8).map(|i| vec![i]).collect();
    assert_eq!(chain_id_ca_certs::<Vec<u8>>(&[]), ChainCertificates::Empty);
    assert_eq!(
        chain_id_ca_certs(&c[..1]),
        ChainCertificates::SelfSigned(&[0])
    );
    for n in 2..=4 {
        assert_eq!(
            chain_id_ca_certs(&c[..n]),
            ChainCertificates::Valid {
                leaf: &[0],
                id: &[1]
            }
        );
    }
    assert_eq!(chain_id_ca_certs(&c), ChainCertificates::Long);
}
