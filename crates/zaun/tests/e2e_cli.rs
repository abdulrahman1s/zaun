//! `e2e_cli` (TESTING_STRATEGY §10.1, TEST-010) and the launch-latency audit of
//! PERFORMANCE §9 (PERF-004).
//!
//! Drives the real binary headless through `runtimes adopt`, `add`, `list`, `sandbox print`,
//! `launch`, `launch --wait`, `status` and `stop` against `fake-umu-run`, a fake Steam Linux
//! Runtime and a fake Proton build in a throw-away home. `bwrap` is `fake-bwrap` (no
//! namespaces needed) unless `ZAUN_E2E=1`, which uses the real one from `PATH`.
//!
//! Timing: the orchestrator logs its segments (`launch hand-off validate_ms=… resolve_ms=…
//! plan_ms=… handoff_ms=…`); a wrapper around `bwrap` records when the supervisor executed
//! it, so the test also measures "command started → bwrap spawned" (budget 300 ms). Debug
//! builds get a generous tolerance; the budgets apply as written to optimised builds.

#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use zaun_testkit::TestEnv;
use zaun_testkit::bins::workspace_bin;
use zaun_testkit::fixtures::pe::sample_game;

const BIN: &str = env!("CARGO_BIN_EXE_zaun");

/// Budgets of PERFORMANCE §9 in milliseconds and the tolerance factor for this build.
const VALIDATE_MS: u64 = 20;
const RESOLVE_MS: u64 = 5;
const PLAN_MS: u64 = 5;
const HANDOFF_MS: u64 = 40;
const OPEN_TO_BWRAP_MS: u64 = 300;

fn tolerance() -> u64 {
    if cfg!(debug_assertions) { 8 } else { 2 }
}

fn real_bwrap() -> bool {
    std::env::var_os("ZAUN_E2E").is_some_and(|v| v == "1")
}

fn write(p: &Path, data: impl AsRef<[u8]>) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, data).unwrap();
}

fn exec(p: &Path, text: &str) {
    write(p, text);
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(name))
            .find(|c| c.is_file())
    })
}

struct World {
    env: TestEnv,
    proton: PathBuf,
    /// A listening socket standing in for the compositor (the planner refuses a launch
    /// without a display).
    _display: std::os::unix::net::UnixListener,
    stamp: PathBuf,
    exe: PathBuf,
    extra: Vec<(String, PathBuf)>,
}

impl World {
    fn new() -> Self {
        let env = TestEnv::new().unwrap();
        let home = env.home();
        // umu's own steamrt3, adopted into the store (TECHNICAL_RESEARCH E10).
        let v = home.join(".local/share/umu/steamrt3");
        write(
            &v.join("sniper_platform_3.0.1/files/lib/libx.so"),
            vec![7u8; 4096],
        );
        write(&v.join("sniper_platform_3.0.1/files/.ref"), "");
        exec(&v.join("_v2-entry-point"), "#!/bin/sh\n");
        symlink("_v2-entry-point", v.join("umu")).unwrap();
        write(&v.join("VERSIONS.txt"), "depot\t3.0.1\n");
        write(&v.join(".installed.ok"), "ok\n");
        exec(&home.join(".local/share/umu/umu-shim"), "#!/bin/sh\n");
        // An external Proton build needing sniper.
        let proton = home.join(".local/share/Steam/compatibilitytools.d/UMU-Proton-10.0-4");
        write(
            &proton.join("toolmanifest.vdf"),
            "\"manifest\"\n{\n  \"version\" \"2\"\n  \"commandline\" \"/proton %verb%\"\n  \"require_tool_appid\" \"1628350\"\n  \"use_sessions\" \"1\"\n  \"compatmanager_layer_name\" \"proton\"\n}\n",
        );
        write(
            &proton.join("compatibilitytool.vdf"),
            "\"compatibilitytools\"\n{\n  \"compat_tools\"\n  {\n    \"UMU-Proton-10.0-4\"\n    {\n      \"install_path\" \".\"\n      \"display_name\" \"UMU-Proton-10.0-4\"\n      \"from_oslist\" \"windows\"\n      \"to_oslist\" \"linux\"\n    }\n  }\n}\n",
        );
        exec(&proton.join("proton"), "#!/bin/sh\n");
        std::fs::create_dir_all(proton.join("files/bin")).unwrap();
        write(
            &proton.join("protonfixes/umu-database.csv"),
            "TITLE,STORE,CODENAME,UMU_ID\n",
        );
        // The game.
        let exe = env.root().join("games/Latency Test/game.exe");
        write(&exe, sample_game());
        // bwrap behind a wrapper that stamps the exec time.
        let stamp = env.root().join("bwrap.stamp");
        let bwrap = if real_bwrap() {
            which("bwrap").expect("ZAUN_E2E=1 needs bwrap on PATH")
        } else {
            workspace_bin("fake-bwrap")
        };
        let date = which("date").expect("date on PATH");
        let wrapper = env.root().join("bin/bwrap");
        exec(
            &wrapper,
            &format!(
                "#!/bin/sh\ncase \"$*\" in *--args*) {} +%s%N >> '{}';; esac\nexec '{}' \"$@\"\n",
                date.display(),
                stamp.display(),
                bwrap.display()
            ),
        );
        let display =
            std::os::unix::net::UnixListener::bind(env.run().join("wayland-test")).unwrap();
        let extra = vec![
            ("WAYLAND_DISPLAY".to_owned(), PathBuf::from("wayland-test")),
            ("ZAUN_BWRAP".to_owned(), wrapper),
            ("ZAUN_UMU_RUN".to_owned(), workspace_bin("fake-umu-run")),
            ("ZAUN_INIT".to_owned(), workspace_bin("zaun-init")),
        ];
        Self {
            env,
            proton,
            _display: display,
            stamp,
            exe,
            extra,
        }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(BIN);
        c.args(args)
            .env_clear()
            .envs(self.env.envs())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("ZAUN_NO_SINGLE_INSTANCE", "1")
            .envs(self.extra.iter().map(|(k, v)| (k, v)));
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        let out = self.cmd(args).output().unwrap();
        assert!(
            out.status.success(),
            "zaun {args:?}: {}\nstdout: {}\nstderr: {}\nlog:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
            self.log()
        );
        out
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.env.state().join("logs/zaun.log")).unwrap_or_default()
    }

    fn stamps(&self) -> Vec<u128> {
        std::fs::read_to_string(&self.stamp)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().parse().ok())
            .collect()
    }
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

/// The `key=value` fields of the last `launch hand-off` line.
fn handoff(log: &str) -> Vec<(String, u64)> {
    let line = log
        .lines()
        .rev()
        .find(|l| l.contains("launch hand-off"))
        .unwrap_or_else(|| panic!("no hand-off line in\n{log}"));
    line.split_whitespace()
        .filter_map(|w| {
            let (k, v) = w.split_once('=')?;
            Some((k.to_owned(), v.trim_matches('"').parse().ok()?))
        })
        .collect()
}

fn field(f: &[(String, u64)], k: &str) -> u64 {
    f.iter()
        .find(|(n, _)| n == k)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("{k} missing from {f:?}"))
}

#[test]
fn cli_launch_pipeline_and_latency() {
    let w = World::new();
    w.run(&["runtimes", "adopt", "--variant", "steamrt3"]);
    let exe = w.exe.to_str().unwrap();
    let proton = w.proton.to_str().unwrap().to_owned();
    w.run(&[
        "add",
        exe,
        "--name",
        "Latency Test",
        "--runtime",
        &proton,
        "--no-launch",
    ]);
    let list: serde_json::Value =
        serde_json::from_slice(&w.run(&["list", "--json"]).stdout).unwrap();
    let id = list["games"]
        .as_array()
        .and_then(|a| a.iter().find(|g| g["name"] == "Latency Test"))
        .and_then(|g| g["id"].as_str())
        .unwrap_or_else(|| panic!("game missing from {list}"))
        .to_owned();
    // `fake-umu-run` creates no prefix; an existing one skips the Prepare session.
    let prefix = w.env.data().join("games").join(&id).join("prefix");
    write(&prefix.join("system.reg"), "WINE REGISTRY Version 2\n");
    std::fs::create_dir_all(prefix.join("drive_c")).unwrap();
    // `sandbox print` takes the program (or a JSON game fixture), like E8.
    let print = String::from_utf8(w.run(&["sandbox", "print", exe]).stdout).unwrap();
    assert!(print.contains("--unshare-"), "{print}");

    // First launch: may audit the adopted runtime; waits for the end.
    let out = w.run(&["launch", "Latency Test", "--wait"]);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(text.contains("started Latency Test"), "{text}");

    // Second launch: runtime installed, audit done (the budgeted case).
    let t0 = now_ns();
    let before = w.stamps().len();
    w.run(&["launch", "Latency Test", "--wait"]);
    let f = handoff(&w.log());
    let tol = tolerance();
    let mut over = Vec::new();
    for (k, budget) in [
        ("validate_ms", VALIDATE_MS),
        ("resolve_ms", RESOLVE_MS),
        ("plan_ms", PLAN_MS),
        ("handoff_ms", HANDOFF_MS),
    ] {
        let v = field(&f, k);
        println!("{k:<12} {v:>5} ms  (budget {budget} ms)");
        if v > budget * tol {
            over.push(format!("{k} = {v} ms > {budget} ms × {tol}"));
        }
    }
    // No audit is pending on the second launch: the gate only reads the audit record.
    let audit = field(&f, "audit_ms");
    println!("audit_ms     {audit:>5} ms  (budget 0 ms, no audit pending)");
    if audit > 5 * tol {
        over.push(format!("audit gate {audit} ms"));
    }
    let stamps = w.stamps();
    assert!(stamps.len() > before, "bwrap was not executed");
    let to_bwrap = Duration::from_nanos((stamps[before] - t0) as u64);
    println!(
        "command start → bwrap exec {:>5} ms  (budget {OPEN_TO_BWRAP_MS} ms)",
        to_bwrap.as_millis()
    );
    if to_bwrap.as_millis() as u64 > OPEN_TO_BWRAP_MS * tol {
        over.push(format!("command → bwrap {to_bwrap:?}"));
    }
    assert!(over.is_empty(), "over budget: {over:?}");

    // status and stop on an idle library.
    let status = String::from_utf8(w.run(&["status"]).stdout).unwrap();
    assert!(!status.is_empty());
    let out = w.cmd(&["stop", "Latency Test"]).output().unwrap();
    assert!(out.status.code().is_some(), "{out:?}");
}

/// A running `zaun service`, killed when dropped (also when the test fails).
struct Service(std::process::Child);

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Start `zaun service` (the resident service, ADR-0022) and wait until it serves.
fn start_service(w: &World) -> Service {
    let mut c = Command::new(BIN);
    c.arg("service")
        .env_clear()
        .envs(w.env.envs())
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        // The display client libraries (the dev shell's), so the service gets as far as
        // trying to open a window on the stand-in compositor.
        .env(
            "LD_LIBRARY_PATH",
            std::env::var_os("LD_LIBRARY_PATH").unwrap_or_default(),
        )
        .envs(w.extra.iter().map(|(k, v)| (k, v)))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let child = Service(c.spawn().unwrap());
    let t = std::time::Instant::now();
    while !w.log().contains("service ready") {
        assert!(
            t.elapsed() < Duration::from_secs(30),
            "service did not start:\n{}",
            w.log()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    child
}

/// A command against the running service (single instance on, unlike [`World::cmd`]).
fn forward(w: &World, args: &[&str]) -> Output {
    let out = Command::new(BIN)
        .args(args)
        .env_clear()
        .envs(w.env.envs())
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .envs(w.extra.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "zaun {args:?}: {}\n{}\nlog:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
        w.log()
    );
    out
}

/// Wait for the `n`-th bwrap stamp; returns it.
fn stamp_after(w: &World, n: usize) -> u128 {
    let t = std::time::Instant::now();
    loop {
        if let Some(s) = w.stamps().get(n) {
            return *s;
        }
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "bwrap was not executed\n{}",
            w.log()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The resident service: `launch` and a file-manager `open` of a known game are handed over
/// and start the sandbox without a cold start; the service outlives its requests.
#[test]
fn resident_service_starts_games_at_once() {
    let w = World::new();
    w.run(&["runtimes", "adopt", "--variant", "steamrt3"]);
    let exe = w.exe.to_str().unwrap().to_owned();
    let proton = w.proton.to_str().unwrap().to_owned();
    w.run(&[
        "add",
        &exe,
        "--name",
        "Latency Test",
        "--runtime",
        &proton,
        "--no-launch",
    ]);
    let list: serde_json::Value =
        serde_json::from_slice(&w.run(&["list", "--json"]).stdout).unwrap();
    let id = list["games"][0]["id"].as_str().unwrap().to_owned();
    let prefix = w.env.data().join("games").join(&id).join("prefix");
    write(&prefix.join("system.reg"), "WINE REGISTRY Version 2\n");
    std::fs::create_dir_all(prefix.join("drive_c")).unwrap();
    // A first launch audits the adopted runtime; the measured ones below do not.
    w.run(&["launch", "Latency Test", "--wait"]);

    let mut service = start_service(&w);
    let tol = tolerance();
    let mut over = Vec::new();
    let mut check = |label: &str, before: usize, t0: u128| {
        let ms = (stamp_after(&w, before) - t0) / 1_000_000;
        println!("{label:<22} → bwrap exec {ms:>5} ms  (budget {OPEN_TO_BWRAP_MS} ms)");
        if ms as u64 > OPEN_TO_BWRAP_MS * tol {
            over.push(format!("{label}: {ms} ms"));
        }
    };

    let before = w.stamps().len();
    let t0 = now_ns();
    let out = forward(&w, &["open", &exe]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("handed over"));
    check("open (file manager)", before, t0);

    // The service sees the short fake session end a moment later; until then `launch` is
    // refused ("already running": exit 3 at once, or after the hand-over). Retry until a
    // sandbox starts and time the attempt that started it.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let (before, t0) = loop {
        assert!(
            std::time::Instant::now() < deadline,
            "launch never started\n{}",
            w.log()
        );
        let before = w.stamps().len();
        let t0 = now_ns();
        let out = w
            .cmd(&["launch", "Latency Test"])
            .env_remove("ZAUN_NO_SINGLE_INSTANCE")
            .output()
            .unwrap();
        match out.status.code() {
            Some(0) => assert!(String::from_utf8_lossy(&out.stdout).contains("handed over")),
            Some(3) => {}
            _ => panic!("launch: {out:?}\n{}", w.log()),
        }
        let t = std::time::Instant::now();
        while w.stamps().len() == before && t.elapsed() < Duration::from_millis(500) {
            std::thread::sleep(Duration::from_millis(10));
        }
        if w.stamps().len() > before {
            break (before, t0);
        }
    };
    check("launch (handed over)", before, t0);
    assert!(
        service.0.try_wait().unwrap().is_none(),
        "the service exited:\n{}",
        w.log()
    );
    assert!(over.is_empty(), "over budget: {over:?}");
}
