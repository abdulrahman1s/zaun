//! The VDF reader (`toolmanifest.vdf`, `compatibilitytool.vdf`) on arbitrary bytes: Proton
//! builds and runtimes found on the machine are untrusted input (THREAT_MODEL T-18).
#![no_main]

use easyplay_core::runtime::vdf;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = vdf::parse_bytes(data);
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = vdf::parse(text);
        let _ = easyplay_core::runtime::discovery::parse_versions_txt(text);
    }
});
