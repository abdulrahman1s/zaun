//! `steam_appid.txt` parsing on arbitrary bytes.
#![no_main]

use zaun_core::identity::hints::parse_steam_appid;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(id) = parse_steam_appid(data) {
        assert!(id > 0);
    }
});
