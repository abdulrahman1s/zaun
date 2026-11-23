//! The ratchet side of re-sync: both parties exchange fresh params, the smaller key hash receives first.
use super::support::{recv, send};
use zaun_chat::ratchet::{
    CURRENT_E2E_VERSION, PqSupport, Ratchet, RatchetVersions, new_resync_params, ratchet_key_hash,
};

#[test]
fn after_a_resync_both_sides_get_working_ratchets_and_one_sends_eready() {
    for pq in [false, true] {
        let v = CURRENT_E2E_VERSION;
        let (a_priv, a_pub) = new_resync_params(v, PqSupport(pq)).unwrap();
        let (b_priv, b_pub) = new_resync_params(v, PqSupport(pq)).unwrap();
        let versions = RatchetVersions::new(v);
        let a = Ratchet::resync(versions, &a_priv, &b_pub, PqSupport(pq)).unwrap();
        let b = Ratchet::resync(versions, &b_priv, &a_pub, PqSupport(pq)).unwrap();
        assert_ne!(
            a.send_eready, b.send_eready,
            "exactly one side sends EREADY"
        );
        assert_eq!(a.ratchet.associated_data().len(), 112);
        let (mut sender, mut receiver) = if a.send_eready {
            (a.ratchet, b.ratchet)
        } else {
            (b.ratchet, a.ratchet)
        };
        assert!(sender.has_sending_chain() && !receiver.has_sending_chain());
        assert_eq!(recv(&mut receiver, &send(&mut sender, "EREADY")), "EREADY");
        assert_eq!(recv(&mut sender, &send(&mut receiver, "ok")), "ok");
        let (p1, p2) = (a_pub.params(), b_pub.params());
        assert_ne!(
            ratchet_key_hash(&p1.k1, &p1.k2),
            ratchet_key_hash(&p2.k1, &p2.k2)
        );
    }
}
