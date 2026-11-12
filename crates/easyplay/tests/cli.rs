//! Smoke tests of the binary.

#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

use easyplay_testkit::TestEnv;

const BIN: &str = env!("CARGO_BIN_EXE_easyplay");

fn run(env: &TestEnv, args: &[&str]) -> std::process::Output {
    Command::new(BIN)
        .args(args)
        .env_clear()
        .envs(env.envs())
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()
        .unwrap()
}

#[test]
fn version_prints_version_and_commit() {
    let env = TestEnv::new().unwrap();
    let out = run(&env, &["--version"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.starts_with(&format!("easyplay {} (", env!("CARGO_PKG_VERSION"))),
        "{text}"
    );
    assert!(text.trim_end().ends_with(')'));
}

#[test]
fn help_lists_verbs_but_not_supervise() {
    let env = TestEnv::new().unwrap();
    let out = run(&env, &["--help"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    for verb in [
        "open", "launch", "stop", "list", "add", "show", "sandbox", "export", "import", "doctor",
        "runtimes",
    ] {
        assert!(text.contains(verb), "{verb} missing from help");
    }
    assert!(!text.contains("supervise"));
}

#[test]
fn verbs_report_not_available_yet() {
    let env = TestEnv::new().unwrap();
    for (args, task) in [
        (&["export", "x"][..], "CLI-003"),
        (&["launch", "x", "--exe", "dx12"][..], "CLI-003"),
        (&["runtimes", "verify"][..], "CLI-002"),
    ] {
        let out = run(&env, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(err.contains(task), "{args:?}: {err}");
        assert!(err.contains("EP-INT-003"), "{err}");
    }
    // The application log was written to the state directory.
    let log = std::fs::read_to_string(env.state().join("logs/easyplay.log")).unwrap();
    assert!(log.contains(" cli["), "{log}");
    assert!(log.contains("command failed"));
}

/// Library verbs on an empty library (the service's headless flows).
#[test]
fn library_verbs_on_an_empty_library() {
    let env = TestEnv::new().unwrap();
    let out = run(&env, &["list"]);
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8(out.stdout).unwrap().contains("No games"));
    let out = run(&env, &["list", "--json"]);
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["games"].as_array().map(Vec::len), Some(0));
    let out = run(&env, &["show", "nothing"]);
    assert_eq!(out.status.code(), Some(1));
}

/// M6 session verbs on an empty library: recovery runs, nothing is running.
#[test]
fn session_verbs_on_an_empty_library() {
    let env = TestEnv::new().unwrap();
    let out = run(&env, &["status"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("no game is running")
    );
    let out = run(&env, &["status", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["format"], 1);
    assert_eq!(v["active"].as_array().unwrap().len(), 0);
    let out = run(&env, &["stop", "nothing-like-this"]);
    assert!(out.status.success());
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("not running")
    );
    let out = run(&env, &["launch", "nothing-like-this"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("EP-LIB-030")
    );
    // A supervisor without a readable plan does not start.
    let out = run(&env, &["supervise", "--plan", "/nonexistent/plan.json"]);
    assert_eq!(out.status.code(), Some(1));
}

/// With no arguments the GUI starts (UI-001); without a display it fails cleanly with the
/// host exit code instead of hanging.
#[test]
fn gui_without_a_display_exits_with_host_error() {
    let env = TestEnv::new().unwrap();
    let out = run(&env, &[]);
    let err = String::from_utf8(out.stderr).unwrap();
    assert_eq!(out.status.code(), Some(4), "{err}");
    assert!(err.contains("EP-HOST-020"), "{err}");
}

#[test]
fn usage_error_exits_2() {
    let env = TestEnv::new().unwrap();
    assert_eq!(run(&env, &["frobnicate"]).status.code(), Some(2));
}
