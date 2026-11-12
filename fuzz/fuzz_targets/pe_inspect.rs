//! Every PE entry point on arbitrary bytes: must return, never panic past the boundary.
#![no_main]

use easyplay_core::identity::pe::{self, PeError, PeLimits};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let limits = PeLimits::default();
    assert_ne!(pe::inspect_bytes(data, &limits).err(), Some(PeError::Panicked));
    assert_ne!(pe::largest_icon_bytes(data, &limits).err(), Some(PeError::Panicked));
    assert_ne!(pe::imports_bytes(data, &limits).err(), Some(PeError::Panicked));
    let _ = pe::format_bytes(data);
});
