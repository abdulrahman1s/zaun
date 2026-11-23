//! Queue rotation, the sender's side: the peer moves its receive queue (`QADD`) and we follow, at agent v8 with SKEY and `QEND`, below with `QKEY`/`QUSE`/`QTEST` (simplexmq 7.1.4 Agent.hs `qAddMsg` … `qEndMsg`, AGPL-3.0-only).
mod addr;
mod answer;
mod codec;
mod cutover;
mod drive;
mod keep;
mod qadd;
mod receive;
mod reseal;
mod state;
mod v7;

pub use state::{Moving, Step, Switch};
