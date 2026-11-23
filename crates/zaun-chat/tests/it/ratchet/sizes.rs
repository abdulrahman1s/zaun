//! Wire sizes follow PQ support, and both directions work.
use super::support::{MODES, PADDED, handshake, recv, send};
use zaun_chat::ratchet::{full_header_len, padded_header_len};

#[test]
fn messages_have_the_padded_sizes_of_their_pq_support_and_flow_both_ways() {
    for (name, invite, join) in MODES {
        let mut p = handshake(invite, join);
        let m1 = send(&mut p.bob, "hello alice");
        let pq = p.bob.pq_support();
        assert_eq!(m1.len(), 54 + padded_header_len(pq) + PADDED, "{name}");
        assert_eq!(m1.len(), 3 + full_header_len(pq) + 16 + PADDED, "{name}");
        assert_eq!(recv(&mut p.alice, &m1), "hello alice");
        let m2 = send(&mut p.alice, "hello bob");
        assert_eq!(recv(&mut p.bob, &m2), "hello bob");
        for i in 0..6 {
            let b = send(&mut p.bob, &format!("b{i}"));
            assert_eq!(recv(&mut p.alice, &b), format!("b{i}"));
            let a = send(&mut p.alice, &format!("a{i}"));
            assert_eq!(recv(&mut p.bob, &a), format!("a{i}"));
        }
        let both = invite && join;
        assert_eq!(
            p.alice.pq_sending().0 && p.alice.pq_receiving().0,
            both,
            "{name}"
        );
    }
}

#[test]
fn the_header_is_padded_to_88_without_pq_and_2310_with_it() {
    assert_eq!(padded_header_len(zaun_chat::ratchet::PqSupport::OFF), 88);
    assert_eq!(padded_header_len(zaun_chat::ratchet::PqSupport::ON), 2310);
}
