//! SEC-006: the wording SECURITY_MODEL §10 requires is present, and no forbidden claim
//! appears in the UI, the README or the shipped desktop files.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use easyplay_ui::strings::{details, settings, setup};

/// SECURITY_MODEL §10: "100 % safe", "cannot escape", "full isolation", "virus-proof".
const FORBIDDEN: &[&str] = &[
    "100 % safe",
    "100% safe",
    "cannot escape",
    "can't escape",
    "full isolation",
    "fully isolated",
    "virus-proof",
    "virus proof",
];

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p
            .extension()
            .is_some_and(|x| ["rs", "md", "desktop", "xml", "in"].iter().any(|e| x == *e))
        {
            out.push(p);
        }
    }
}

#[test]
fn no_forbidden_claims() {
    let ui = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = ui.parent().and_then(Path::parent).unwrap();
    let mut all = Vec::new();
    files(&ui.join("src"), &mut all);
    files(&root.join("crates/easyplay/src"), &mut all);
    files(&root.join("assets"), &mut all);
    all.push(root.join("README.md"));
    assert!(all.len() > 20, "scanned too few files: {}", all.len());
    for f in all {
        let text = std::fs::read_to_string(&f)
            .unwrap_or_default()
            .to_lowercase();
        for claim in FORBIDDEN {
            assert!(
                !text.contains(claim),
                "{} contains \"{claim}\"",
                f.display()
            );
        }
    }
}

#[test]
fn required_wording_is_present() {
    // Setup dialog: the honesty line.
    assert!(
        setup::HONESTY.contains("reduces risk"),
        "{}",
        setup::HONESTY
    );
    assert!(setup::HONESTY.contains("cannot make untrusted software"));
    // Online and Standard mention the local network.
    assert!(details::INTERNET_ON.contains("local network"));
    // X11 notice.
    assert!(details::X11_NOTICE.contains("X11") && details::X11_NOTICE.contains("not isolated"));
    // Shared runtime mode: one sentence on the Protection settings page.
    assert!(settings::ISOLATION_SHARED.contains("shared mode"));
    assert!(settings::ISOLATION_SHARED.contains("checked after each game"));
}

/// SECURITY_MODEL §10, "Sound toggle: mentions the microphone". Open: the toggle label is
/// "Sound" only (THREAT_MODEL T-14 residual risk); the UI owner changes the string and
/// removes the `ignore`.
#[test]
#[ignore = "SEC-006 open: the sound toggle does not mention the microphone yet"]
fn sound_toggle_mentions_the_microphone() {
    assert!(
        details::SOUND.to_lowercase().contains("microphone"),
        "{}",
        details::SOUND
    );
}
