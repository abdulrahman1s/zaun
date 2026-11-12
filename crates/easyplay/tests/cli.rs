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
        (&["list"][..], "CLI-002"),
        (&["launch", "x"][..], "CLI-003"),
        (&["open", "/tmp/x.exe"][..], "DESKTOP-002"),
        (&["supervise", "--plan", "/tmp/p.json"][..], "PROC-003"),
        (&[][..], "UI-001"),
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

#[test]
fn usage_error_exits_2() {
    let env = TestEnv::new().unwrap();
    assert_eq!(run(&env, &["frobnicate"]).status.code(), Some(2));
}
