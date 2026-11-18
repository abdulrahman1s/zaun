//! The session-output classifier on arbitrary lines: the game controls its own output
//! (probe P-28, THREAT_MODEL T-20).
#![no_main]

use zaun_core::umu::log_parse::LogClassifier;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let mut c = LogClassifier::new().with_runtime_store(std::path::Path::new("/store"));
    for (i, line) in text.split('\n').enumerate() {
        if i == 3 {
            c.set_sandbox_started();
        }
        let _ = c.feed(line);
    }
});
