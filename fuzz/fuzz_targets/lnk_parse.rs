//! The shortcut parser on arbitrary bytes.
#![no_main]

use zaun_core::identity::lnk;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(t) = lnk::parse(data)
        && let Some(p) = t.local_path.as_deref()
    {
        let _ = lnk::windows_to_prefix(std::path::Path::new("/pfx"), p);
    }
});
