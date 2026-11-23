//! A stored SEND body moved from one send queue to another: pending messages are sealed with the old queue's e2e key when queued, where 7.1.4 seals at send time (Agent/Client.hs `agentCbEncrypt`).
use crate::crypto::cb_decrypt;
use crate::encoding::Decode;

use crate::agent::delivery::{ClientMessage, ClientMsgEnvelope, seal};
use crate::agent::error::Result;
use crate::agent::records::SndQueue;

/// Opens our own box with `old`'s secret and seals what was inside for `new`. The agent envelope stays
/// byte for byte the same, so no ratchet step is taken twice.
pub(crate) fn reseal(old: &SndQueue, new: &SndQueue, body: &[u8]) -> Result<Vec<u8>> {
    let env = ClientMsgEnvelope::from_bytes(body)?;
    let plain = cb_decrypt(&old.e2e_secret()?, &env.nonce, &env.enc_body)?;
    let msg = ClientMessage::from_bytes(&plain)?;
    seal(new, &msg.body, env.e2e_pub.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::delivery::open;
    use crate::agent::records::RcvQueue;
    use crate::crypto::{Alg, KeyPair};
    use crate::encoding::Version;

    /// The peer's receive queue and our send queue to it, with matching e2e keys.
    fn pair() -> (RcvQueue, SndQueue) {
        let key = || KeyPair::generate(Alg::X25519).unwrap();
        let (peer, ours) = (key(), key());
        let server: crate::smp::SmpServer =
            "smp://AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=@fake.test"
                .parse()
                .unwrap();
        let rq = RcvQueue {
            server: server.clone(),
            rcv_id: vec![1; 24],
            snd_id: vec![2; 24],
            auth: KeyPair::generate(Alg::Ed25519).unwrap().private,
            dh_priv: key().private,
            server_dh: None,
            e2e_priv: peer.private,
            peer_e2e: Some(ours.public),
            client_version: Version(4),
        };
        let sq = SndQueue {
            server,
            snd_id: vec![2; 24],
            auth: key().private,
            e2e_priv: ours.private,
            peer_dh: peer.public,
            client_version: Version(4),
            secured: true,
        };
        (rq, sq)
    }

    #[test]
    fn a_message_sealed_for_the_old_queue_opens_on_the_new_one_after_resealing_and_not_before() {
        let (old_rq, old) = pair();
        let (new_rq, new) = pair();
        let body = seal(&old, b"the agent envelope", false).unwrap();
        assert!(open(&new_rq, &body).is_err());
        let moved = reseal(&old, &new, &body).unwrap();
        assert_eq!(
            open(&new_rq, &moved).unwrap().envelope,
            b"the agent envelope"
        );
        assert!(open(&old_rq, &moved).is_err());
        assert_eq!(moved.len(), body.len());
    }
}
