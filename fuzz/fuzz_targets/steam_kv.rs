//! Steam's KeyValues files (`libraryfolders.vdf`, `appmanifest_*.acf`, `loginusers.vdf`,
//! `localconfig.vdf`, `config.vdf`) are written by another program and parsed by trusted code
//! (THREAT_MODEL T-22, ADR-0031): the tolerant reader and every reader built on it must never
//! panic, loop or allocate without bound, on any bytes.
#![no_main]

use zaun_core::steam::keyvalues::{self, KvLimits};
use zaun_core::steam::{account, compat, manifest, roots};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = keyvalues::parse(data, &KvLimits::SMALL);
    let _ = keyvalues::parse(
        data,
        &KvLimits {
            max_bytes: 4096,
            max_depth: 3,
            max_entries: 16,
            max_token_bytes: 8,
        },
    );
    let _ = roots::parse_library_folders(data);
    let _ = manifest::parse_manifest(data);
    let _ = account::most_recent_account(data);
    let _ = account::play_times(data);
    let _ = compat::tool_mapping(data);
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = compat::proton_dir_from_config_info(text);
        let _ = manifest::manifest_app_id(text);
        let _ = zaun_core::steam::client::launched_app_id(
            &text.split(' ').map(std::ffi::OsString::from).collect::<Vec<_>>(),
        );
    }
});
