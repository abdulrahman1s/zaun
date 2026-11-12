//! The status descriptors (`bwrap --json-status-fd`, the init shim's `--status-fd`) on
//! arbitrary bytes split at an arbitrary point: both are written from inside the sandbox
//! (PROC-002, THREAT_MODEL T-20).
#![no_main]

use easyplay_core::session::{BwrapStatusParser, InitStatusParser};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let cut = data.first().map_or(0, |b| usize::from(*b) % (data.len().max(1)));
    let (a, b) = data.split_at(cut);
    let mut bw = BwrapStatusParser::new();
    let _ = bw.push(a);
    let _ = bw.push(b);
    let mut init = InitStatusParser::new();
    let _ = init.push(a);
    let _ = init.push(b);
});
