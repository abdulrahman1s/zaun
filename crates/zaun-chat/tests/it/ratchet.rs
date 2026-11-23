//! Tests of the double ratchet (src/ratchet): ported from nook's ratchet.test.ts and simplexmq's RatchetTests.hs, plus known answers from nook and the reference C KEM.

mod kem_vectors;
mod legacy;
mod limits;
mod nook;
mod order;
mod params;
mod persist;
mod pq_switch;
mod resync;
mod sizes;
mod support;
mod two_phase;
mod x3dh;
