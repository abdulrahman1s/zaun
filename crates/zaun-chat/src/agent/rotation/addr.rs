//! Naming a send queue: by address as the rotation messages do (Agent/Protocol.hs `SndQAddr`, `sameQAddress`), and by tag for the switch's own requests.
use crate::agent::message::SndQAddr;
use crate::agent::records::SndQueue;

/// The first bytes of the queue's random sender id.
#[must_use]
pub fn switch_tag(sq: &SndQueue) -> u64 {
    let mut b = [0u8; 8];
    for (to, from) in b.iter_mut().zip(&sq.snd_id) {
        *to = *from;
    }
    u64::from_be_bytes(b)
}

/// `sameQAddress`: the server's hosts and port, and the sender id.
#[must_use]
pub fn same_addr(sq: &SndQueue, a: &SndQAddr) -> bool {
    sq.server.hosts == a.server.hosts && sq.server.port == a.server.port && sq.snd_id == a.sender_id
}

#[must_use]
pub fn addr(sq: &SndQueue) -> SndQAddr {
    SndQAddr {
        server: sq.server.clone(),
        sender_id: sq.snd_id.clone(),
    }
}
