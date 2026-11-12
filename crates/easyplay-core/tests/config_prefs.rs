//! Preferences: defaults, validation, unknown keys, atomic save (CFG-001…CFG-003).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use easyplay_core::clock::Timestamp;
use easyplay_core::config::load::{self, ConfigProblem, ConfigSource, EnvRule, LoadOptions};
use easyplay_core::config::schema::{Theme, WeekStart};
use easyplay_core::config::{PreferencePatch, Preferences, RuntimeDefault};
use easyplay_core::fsx::{Fs, RealFs};
use easyplay_core::model::BasePreset;

fn now() -> Timestamp {
    Timestamp::from_millis(1_794_480_000_000)
}

fn documented_file() -> String {
    let doc = include_str!("../../../docs/CONFIGURATION.md");
    let start = doc.find("## 2. `config.toml`").unwrap();
    let rest = &doc[start..];
    let open = rest.find("```toml\n").unwrap() + "```toml\n".len();
    let close = rest[open..].find("```").unwrap();
    rest[open..open + close].to_owned()
}

fn write(path: &Path, text: &str) {
    RealFs::new()
        .atomic_write(path, text.as_bytes(), 0o600)
        .unwrap();
}

fn read(path: &Path) -> String {
    String::from_utf8(RealFs::new().read(path, 1 << 20).unwrap()).unwrap()
}

#[test]
fn defaults_serialise_to_the_documented_file() {
    let ours: toml::Table = load::to_toml_string(&Preferences::default())
        .unwrap()
        .parse()
        .unwrap();
    let doc: toml::Table = documented_file().parse().unwrap();
    assert_eq!(ours, doc);
    let text = load::to_toml_string(&Preferences::default()).unwrap();
    assert!(text.starts_with(load::HEADER));
    // Documented order is kept.
    assert!(text.find("[general]").unwrap() < text.find("[onboarding]").unwrap());
}

#[test]
fn documented_file_loads_without_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    write(&path, &documented_file());
    let loaded = load::load(&RealFs::new(), &path, now(), &LoadOptions::default()).unwrap();
    assert_eq!(loaded.source, ConfigSource::File);
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert_eq!(loaded.prefs, Preferences::default());
}

#[test]
fn hand_edited_file_keeps_unknown_keys_and_reports_two_invalid_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    write(
        &path,
        r#"# my settings
schema_version = 1
future_flag = true   # unknown top-level key

[appearance]
theme = "DARK"       # case-insensitive
ui_scale = 7.5       # out of range: clamped
accent = "neon"      # invalid: default
sparkles = 3         # unknown key in a known section

[statistics]
week_starts_on = "monday"

[plugins.foo]        # unknown section
enabled = true
"#,
    );
    let fs = RealFs::new();
    let loaded = load::load(&fs, &path, now(), &LoadOptions::default()).unwrap();
    assert_eq!(loaded.warnings.len(), 2, "{:?}", loaded.warnings);
    assert_eq!(loaded.warnings[0].key, "appearance.accent");
    assert_eq!(loaded.warnings[1].key, "appearance.ui_scale");
    assert!(matches!(
        loaded.warnings[1].problem,
        ConfigProblem::Clamped { .. }
    ));
    let p = &loaded.prefs;
    assert_eq!(p.appearance.theme, Theme::Dark);
    assert!((p.appearance.ui_scale - 2.0).abs() < f64::EPSILON);
    assert_eq!(p.statistics.week_starts_on, WeekStart::Monday);
    let mut unknown = loaded.unknown_keys.clone();
    unknown.sort();
    assert_eq!(
        unknown,
        vec!["appearance.sparkles", "future_flag", "plugins"]
    );

    load::save(&fs, &path, p).unwrap();
    let text = read(&path);
    assert!(text.starts_with(load::HEADER));
    assert!(!text.contains("# my settings"));
    let back: toml::Table = text.parse().unwrap();
    assert_eq!(back["future_flag"].as_bool(), Some(true));
    assert_eq!(back["appearance"]["sparkles"].as_integer(), Some(3));
    assert_eq!(back["plugins"]["foo"]["enabled"].as_bool(), Some(true));
    assert_eq!(back["appearance"]["accent"].as_str(), Some("tide"));
    // And it reloads cleanly with the same unknown keys.
    let again = load::load(&fs, &path, now(), &LoadOptions::default()).unwrap();
    assert!(again.warnings.is_empty());
    assert_eq!(again.prefs, *p);
}

#[test]
fn validation_table() {
    let fs = RealFs::new();
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("umu-run");
    fs.atomic_write(&exe, b"#!/bin/sh\n", 0o755).unwrap();
    let plain = dir.path().join("plain");
    fs.atomic_write(&plain, b"", 0o644).unwrap();
    let cases: Vec<(String, usize)> = vec![
        ("[general]\nlanguage = \"en-GB\"".into(), 0),
        ("[general]\nlanguage = \"not a tag!\"".into(), 1),
        ("[general]\nconfirm_remove = \"yes\"".into(), 1),
        ("[protection]\ndefault_preset = \"Online\"".into(), 0),
        ("[protection]\ndefault_preset = \"custom\"".into(), 1),
        ("[runtime]\ndefault = \"pinned:GE-Proton10-25\"".into(), 0),
        ("[runtime]\ndefault = \"external:relative\"".into(), 1),
        ("[runtime]\nauto_update_interval_hours = 1".into(), 1),
        (
            "[runtime]\nextra_search_dirs = [\"/opt/proton\", \"rel\", \"/proc\", 5]".into(),
            3,
        ),
        ("[launch]\nstop_grace_secs = 100".into(), 1),
        (
            "[launch]\nglobal_env = [{ name = \"DXVK_HUD\", value = \"fps\" }]".into(),
            0,
        ),
        (
            "[launch]\nglobal_env = [{ name = \"1BAD\", value = \"x\" }, \"junk\"]".into(),
            2,
        ),
        ("[statistics]\nmin_session_secs = -4".into(), 1),
        ("[logging]\nlevel = \"verbose\"".into(), 1),
        ("[logging]\nsession_log_cap_mb = 0".into(), 1),
        ("[advanced]\nrenderer = \"GLOW\"".into(), 0),
        (format!("[paths]\numu_run = \"{}\"", exe.display()), 0),
        (format!("[paths]\nbwrap = \"{}\"", plain.display()), 1),
        ("[paths]\nbwrap = \"/does/not/exist\"".into(), 1),
        ("[paths]\nbwrap = \"\"".into(), 0),
        ("appearance = 3".into(), 1),
    ];
    for (text, expected) in cases {
        let table: toml::Table = text.parse().unwrap();
        let (_, warnings) = load::from_table(table, &fs, &LoadOptions::default());
        assert_eq!(warnings.len(), expected, "{text}: {warnings:?}");
    }
    let (p, _) = load::from_table(
        "[runtime]\ndefault = \"Pinned:GE-Proton10-25\"\n[protection]\ndefault_preset = \"standard\""
            .parse()
            .unwrap(),
        &fs,
        &LoadOptions::default(),
    );
    assert!(
        matches!(p.runtime.default, RuntimeDefault::Pinned(ref n) if n.as_str() == "GE-Proton10-25")
    );
    assert_eq!(p.protection.default_preset, BasePreset::Standard);
}

#[test]
fn dangerous_variables_require_developer_mode() {
    let fs = RealFs::new();
    let rule = |name: &str| match name {
        "WINEPREFIX" => EnvRule::Reserved,
        "LD_PRELOAD" => EnvRule::Dangerous,
        _ => EnvRule::Allowed,
    };
    let opts = LoadOptions {
        env_rule: Some(&rule),
    };
    let env = "[launch]\nglobal_env = [{ name = \"WINEPREFIX\", value = \"/x\" }, \
               { name = \"LD_PRELOAD\", value = \"/y.so\" }]";
    let (p, w) = load::from_table(env.parse().unwrap(), &fs, &opts);
    assert_eq!((p.launch.global_env.len(), w.len()), (0, 2));
    let (p, w) = load::from_table(
        format!("{env}\n[advanced]\ndeveloper_mode = true")
            .parse()
            .unwrap(),
        &fs,
        &opts,
    );
    assert_eq!((p.launch.global_env.len(), w.len()), (1, 1));
}

#[test]
fn unparsable_file_is_renamed_and_defaults_are_used() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    write(&path, "[general\nthis is = not toml");
    let fs = RealFs::new();
    let loaded = load::load(&fs, &path, now(), &LoadOptions::default()).unwrap();
    assert_eq!(loaded.source, ConfigSource::Unparsable);
    assert_eq!(loaded.prefs, Preferences::default());
    let ConfigProblem::Unparsable {
        moved_to: Some(aside),
        ..
    } = &loaded.warnings[0].problem
    else {
        panic!("{:?}", loaded.warnings);
    };
    assert!(aside.to_str().unwrap().contains("config.toml.invalid-"));
    assert!(fs.exists(aside));
    assert!(!fs.exists(&path));
    assert_eq!(
        loaded.warnings[0].code(),
        easyplay_core::ErrorCode::CfgUnreadable
    );
}

#[test]
fn newer_file_is_left_untouched_and_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let text = "schema_version = 9\n[appearance]\ntheme = \"dark\"\n";
    write(&path, text);
    let loaded = load::load(&RealFs::new(), &path, now(), &LoadOptions::default()).unwrap();
    assert_eq!(loaded.source, ConfigSource::Newer);
    assert!(!loaded.writable());
    assert_eq!(loaded.prefs, Preferences::default());
    assert_eq!(
        loaded.warnings[0].code(),
        easyplay_core::ErrorCode::CfgNewerVersion
    );
    assert_eq!(read(&path), text);
}

#[test]
fn missing_file_gives_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load::load(
        &RealFs::new(),
        &dir.path().join("config.toml"),
        now(),
        &LoadOptions::default(),
    )
    .unwrap();
    assert_eq!(loaded.source, ConfigSource::Missing);
    assert!(loaded.writable());
}

#[test]
fn patches_validate_like_the_file() {
    let fs = RealFs::new();
    let opts = LoadOptions::default();
    let base = Preferences::default();
    let next = base
        .apply(
            &PreferencePatch::new()
                .set("appearance.theme", "light")
                .set("launch.stop_grace_secs", 30),
            &fs,
            &opts,
        )
        .unwrap();
    assert_eq!(next.appearance.theme, Theme::Light);
    assert_eq!(next.launch.stop_grace_secs, 30);
    for (key, value) in [
        ("appearance.theme", toml::Value::from("purple")),
        ("launch.stop_grace_secs", toml::Value::from(1)),
        ("appearance.nonsense", toml::Value::from(true)),
        ("theme", toml::Value::from("dark")),
    ] {
        let err = base
            .apply(&PreferencePatch::new().set(key, value), &fs, &opts)
            .unwrap_err();
        let e: easyplay_core::Error = err.into();
        assert_eq!(e.code, easyplay_core::ErrorCode::CfgInvalidValue);
    }
}
