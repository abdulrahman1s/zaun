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

/// The same claims in Arabic (a catalogue's text is checked against both lists).
const FORBIDDEN_AR: &[&str] = &[
    "آمن تماما",
    "آمنة تماما",
    "آمن تمامًا",
    "آمنة تمامًا",
    "حماية كاملة",
    "عزل كامل",
    "معزولة تماما",
    "معزولة تمامًا",
    "لا يمكنه الهروب",
    "لا يمكنها الهروب",
    "خالٍ من الفيروسات",
    "خالية من الفيروسات",
    "مضمون",
    "مضمونة",
];

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if p.extension().is_some_and(|x| {
            ["rs", "md", "desktop", "xml", "in", "yml"]
                .iter()
                .any(|e| x == *e)
        }) {
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
    // The catalogues hold every user-visible string of the interface (ADR-0032).
    files(&ui.join("locales"), &mut all);
    files(&root.join("crates/easyplay/src"), &mut all);
    files(&root.join("assets"), &mut all);
    all.push(root.join("README.md"));
    assert!(all.len() > 20, "scanned too few files: {}", all.len());
    for f in all {
        let text = std::fs::read_to_string(&f)
            .unwrap_or_default()
            .to_lowercase();
        for claim in FORBIDDEN.iter().chain(FORBIDDEN_AR) {
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
        setup::HONESTY().contains("reduces risk"),
        "{}",
        setup::HONESTY()
    );
    assert!(setup::HONESTY().contains("cannot make untrusted software"));
    // Online and Standard mention the local network.
    assert!(details::INTERNET_ON().contains("local network"));
    // X11 notice.
    assert!(
        details::X11_NOTICE().contains("X11") && details::X11_NOTICE().contains("not isolated")
    );
    // Shared runtime mode: one sentence on the Protection settings page.
    assert!(settings::ISOLATION_SHARED().contains("shared mode"));
    assert!(settings::ISOLATION_SHARED().contains("checked after each game"));
}

/// SECURITY_MODEL §10, "The microphone is named wherever it is granted" (§5.3, ADR-0026):
/// it has a switch of its own, and the sound switch does not claim it.
#[test]
fn the_microphone_is_named_where_it_is_granted() {
    assert!(
        details::MICROPHONE().to_lowercase().contains("microphone"),
        "{}",
        details::MICROPHONE()
    );
    assert!(!details::SOUND().to_lowercase().contains("microphone"));
    assert!(
        details::MICROPHONE_INSEPARABLE()
            .to_lowercase()
            .contains("microphone")
    );
}
