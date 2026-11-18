//! UMU-003/UMU-004: the classifier against the captured corpus in
//! `zaun-testkit/fixtures/logs/` (see its README), plus interleaving.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use zaun_core::fsx::{Fs, RealFs};
use zaun_core::umu::log_parse::FixLevel;
use zaun_core::umu::{LineClass, LogClassifier, NoiseKind, UmuPhase};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../zaun-testkit/fixtures/logs")
}

struct Annotated {
    name: String,
    failure: Option<String>,
    sandbox_started: bool,
    lines: Vec<(String, String)>,
}

fn load(name: &str) -> Annotated {
    let path = corpus_dir().join(format!("{name}.txt"));
    let text = String::from_utf8(RealFs::new().read(&path, 1 << 20).unwrap()).unwrap();
    let mut a = Annotated {
        name: name.to_owned(),
        failure: None,
        sandbox_started: false,
        lines: Vec::new(),
    };
    for l in text.lines() {
        if let Some(v) = l.strip_prefix("# failure: ") {
            a.failure = (v != "none").then(|| v.to_owned());
        } else if let Some(v) = l.strip_prefix("# sandbox_started: ") {
            a.sandbox_started = v == "yes";
        } else if l.starts_with("# ") {
        } else {
            let (tag, line) = l.split_once('\t').expect("tag<TAB>line");
            a.lines.push((tag.to_owned(), line.to_owned()));
        }
    }
    a
}

const FILES: &[&str] = &[
    "offline_game_isolated",
    "container_execvp_failure",
    "inner_bwrap_failure",
    "proton_traceback",
    "umu_invalid_protonpath",
    "umu_no_protonpath",
];

fn tag(c: &LineClass) -> String {
    match c {
        LineClass::Phase(p) => format!(
            "phase:{}",
            match p {
                UmuPhase::Starting { .. } => "starting",
                UmuPhase::Launching { .. } => "launching",
                UmuPhase::UpgradingPrefix => "upgrading_prefix",
                UmuPhase::UsingProton { .. } => "using_proton",
                UmuPhase::UpToDate { .. } => "up_to_date",
                _ => "other",
            }
        ),
        LineClass::Fix { level, .. } => format!(
            "fix:{}",
            match level {
                FixLevel::Info => "info",
                FixLevel::Warn => "warn",
                FixLevel::Error => "error",
            }
        ),
        LineClass::Wine(_) => "wine".into(),
        LineClass::UmuLog(_) => "umu_log".into(),
        LineClass::TracebackStart => "tb_start".into(),
        LineClass::TracebackLine => "tb_line".into(),
        LineClass::UmuError { .. } => "umu_error".into(),
        LineClass::ContainerError(_) => "container_error".into(),
        LineClass::InnerBwrapError(_) => "inner_bwrap".into(),
        LineClass::OuterBwrapError(_) => "outer_bwrap".into(),
        LineClass::Noise(NoiseKind::HardLinkFallback) => "noise:hard_link".into(),
        LineClass::Noise(NoiseKind::LocaleGen) => "noise:locale_gen".into(),
        LineClass::Other => "other".into(),
    }
}

fn classifier(a: &Annotated) -> LogClassifier {
    let mut c = LogClassifier::new();
    if a.sandbox_started {
        c.set_sandbox_started();
    }
    c
}

#[test]
fn corpus_classifies_as_annotated() {
    for name in FILES {
        let a = load(name);
        assert!(!a.lines.is_empty(), "{name}");
        let mut c = classifier(&a);
        for (i, (expected, line)) in a.lines.iter().enumerate() {
            let got = tag(&c.feed(line));
            assert_eq!(&got, expected, "{}:{} {line}", a.name, i + 1);
        }
        let code = c.failure().map(|f| f.code.as_str().to_owned());
        assert_eq!(code, a.failure, "{name}: failure {:?}", c.failure());
    }
}

#[test]
fn corpus_facts() {
    let a = load("offline_game_isolated");
    let mut c = classifier(&a);
    for (_, l) in &a.lines {
        c.feed(l);
    }
    assert_eq!(c.version(), Some("1.4.4"));
    assert!(c.launching_seen());
    assert!(!c.fix_needs_network());
    assert_eq!(
        c.noise_summary(),
        vec![
            "[9 similar lines hidden: pressure-vessel hard-link fallback]",
            "[3 similar lines hidden: pressure-vessel locale generation]",
        ]
    );
    let a = load("umu_invalid_protonpath");
    let mut c = classifier(&a);
    for (_, l) in &a.lines {
        c.feed(l);
    }
    let f = c.failure().unwrap();
    assert!(f.message.contains("toolmanifest.vdf not found"));
    assert!(f.details.len() <= 20 && f.details.last().unwrap().starts_with("FileNotFoundError"));
}

/// Lines of another process inside a traceback do not end it or change its result, and
/// keep their own classification.
#[test]
fn interleaved_traceback() {
    let tb = load("umu_invalid_protonpath");
    let other = load("offline_game_isolated");
    let mut mixed: Vec<(String, String)> = Vec::new();
    let mut others = other
        .lines
        .iter()
        .filter(|(t, l)| !l.starts_with(' ') && t != "noise:hard_link")
        .cycle();
    for (t, l) in &tb.lines {
        mixed.push((t.clone(), l.clone()));
        if t == "tb_line" {
            let (ot, ol) = others.next().unwrap();
            mixed.push((ot.clone(), ol.clone()));
        }
    }
    let mut c = LogClassifier::new();
    c.set_sandbox_started();
    for (expected, line) in &mixed {
        assert_eq!(&tag(&c.feed(line)), expected, "{line}");
    }
    assert_eq!(c.failure().unwrap().code.as_str(), "ZN-RT-012");
}

/// Adversarial game output imitating patterns without the expected prefix context is game
/// output (documented limits in `umu::log_parse`).
#[test]
fn adversarial_lines_are_game_output() {
    let mut c = LogClassifier::new();
    for l in [
        "game: INFO: Running 'x' using runtime 'y'",
        " INFO: umu-launcher version 9",
        "Using GE-Proton",
        "[game] ERROR: oh no",
        "pressure-vessel-wrap: E: fake",
        "pressure-vessel-wrap[12a]: E: fake",
        "ProtonFixes[x] INFO: fake",
        "NotAnError: message",
    ] {
        assert_eq!(tag(&c.feed(l)), "other", "{l}");
    }
    assert!(c.failure().is_none());
}
