//! UMU-005: contract tests against the real `umu-run` (UMU_INTEGRATION §10), run only with
//! `EASYPLAY_E2E=1` and `EASYPLAY_UMU_RUN` pointing at the program to check.
//!
//! Present: `version_line`. The launch-level rows of §10 (`offline_launch_readonly_proton`,
//! `isolated_runtime_launch`, `prepare_creates_prefix`, `phase_lines`) are exercised by the
//! manual first-run experiment of TECHNICAL_RESEARCH §6 (E11) through `easyplay launch`;
//! automating them needs a cached runtime on the CI runner (TEST-010).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use easyplay_core::host::umu_probe::parse_umu_version;

fn umu_run() -> Option<std::path::PathBuf> {
    if std::env::var_os("EASYPLAY_E2E").is_none_or(|v| v != "1") {
        return None;
    }
    std::env::var_os("EASYPLAY_UMU_RUN").map(Into::into)
}

#[test]
fn version_line() {
    let Some(umu) = umu_run() else {
        return;
    };
    let out = std::process::Command::new(&umu)
        .arg("--version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", std::env::temp_dir())
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    let v = parse_umu_version(&text).unwrap_or_else(|| panic!("unparsed: {text:?}"));
    assert!(v.major >= 1, "{v:?}");
}
