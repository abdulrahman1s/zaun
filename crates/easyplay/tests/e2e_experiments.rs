//! TEST-020: the real-host experiments of TECHNICAL_RESEARCH §6 (E8–E12) as automated tests.
//!
//! They run the real binary with real `bwrap`, a real `umu-run`, a real Proton build and the
//! Steam Linux Runtime, entirely in a throw-away directory under `CARGO_TARGET_TMPDIR`
//! (every `EASYPLAY_*_DIR`, `HOME` and `XDG_RUNTIME_DIR`), and check that the user's own
//! umu data is left untouched.
//!
//! | Variable | Meaning |
//! |---|---|
//! | `EASYPLAY_E2E=1` | run at all (needs unprivileged user namespaces and `bwrap` on `PATH`) |
//! | `EASYPLAY_UMU_RUN` | the `umu-run` to test (on NixOS: the FHS wrapper, not a sandboxing wrapper) |
//! | `EASYPLAY_E2E_PROTON` | a Proton build directory (default `~/.local/share/Steam/compatibilitytools.d/UMU-Proton-10.0-4`) |
//! | `EASYPLAY_E2E_UMU_DIR` | umu's data directory holding `steamrt3` (default `~/.local/share/umu`), adopted with `runtimes adopt` instead of downloading |
//! | `EASYPLAY_E2E_NET=1` | also run E12, the first run from an empty store (downloads about 2 GB) |
//!
//! The programs are copies of Proton's own `cmd.exe` (64-bit and 32-bit), as in the manual
//! runs. Without the variables every test returns early (and says so).

#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use easyplay_testkit::bins::workspace_bin;

const BIN: &str = env!("CARGO_BIN_EXE_easyplay");

struct Inputs {
    umu_run: PathBuf,
    proton: PathBuf,
    umu_dir: PathBuf,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn inputs() -> Option<Inputs> {
    if std::env::var_os("EASYPLAY_E2E").is_none_or(|v| v != "1") {
        eprintln!("skipped: EASYPLAY_E2E=1 not set");
        return None;
    }
    let Some(umu_run) = std::env::var_os("EASYPLAY_UMU_RUN").map(PathBuf::from) else {
        eprintln!("skipped: EASYPLAY_UMU_RUN not set");
        return None;
    };
    let proton = std::env::var_os("EASYPLAY_E2E_PROTON").map_or_else(
        || home().join(".local/share/Steam/compatibilitytools.d/UMU-Proton-10.0-4"),
        PathBuf::from,
    );
    let umu_dir = std::env::var_os("EASYPLAY_E2E_UMU_DIR")
        .map_or_else(|| home().join(".local/share/umu"), PathBuf::from);
    for (what, p) in [
        ("Proton build", proton.join("proton")),
        ("steamrt3", umu_dir.join("steamrt3")),
    ] {
        if !p.exists() {
            eprintln!("skipped: {what} not found at {}", p.display());
            return None;
        }
    }
    Some(Inputs {
        umu_run,
        proton,
        umu_dir,
    })
}

/// A fingerprint of a directory tree (names, sizes, modification times), to prove it was
/// not changed.
fn tree_fingerprint(dir: &Path, depth: usize) -> BTreeMap<PathBuf, (u64, i64)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, n)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(md) = std::fs::symlink_metadata(e.path()) else {
                continue;
            };
            out.insert(e.path(), (md.size(), md.mtime()));
            if md.is_dir() && n < depth {
                stack.push((e.path(), n + 1));
            }
        }
    }
    out
}

/// A throw-away EasyPlay installation.
struct Lab {
    _dir: tempfile::TempDir,
    root: PathBuf,
    inputs: Inputs,
}

impl Lab {
    fn new(inputs: Inputs, adopt: bool) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("e2e-experiments-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["home", "config", "data", "cache", "state", "run", "games"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::set_permissions(
            root.join("run"),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let lab = Self {
            _dir: dir,
            root,
            inputs,
        };
        // Proton is found through the search directories of the preferences, read-only.
        let search = lab.inputs.proton.parent().unwrap().display().to_string();
        std::fs::write(
            lab.root.join("config/config.toml"),
            format!(
                "[runtime]\nextra_search_dirs = [\"{search}\"]\n\n[onboarding]\ncompleted = true\n"
            ),
        )
        .unwrap();
        if adopt {
            // `runtimes adopt` reads umu's data directory under `$HOME`.
            let share = lab.root.join("home/.local/share");
            std::fs::create_dir_all(&share).unwrap();
            symlink(&lab.inputs.umu_dir, share.join("umu")).unwrap();
            lab.ok(&["runtimes", "adopt", "--variant", "steamrt3"]);
        }
        lab
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(BIN);
        c.args(args)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.root.join("home"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("EASYPLAY_CONFIG_DIR", self.root.join("config"))
            .env("EASYPLAY_DATA_DIR", self.root.join("data"))
            .env("EASYPLAY_CACHE_DIR", self.root.join("cache"))
            .env("EASYPLAY_STATE_DIR", self.root.join("state"))
            .env("EASYPLAY_UMU_RUN", &self.inputs.umu_run)
            .env("EASYPLAY_INIT", workspace_bin("easyplay-init"))
            .env("EASYPLAY_NO_SINGLE_INSTANCE", "1")
            .stdin(Stdio::null());
        // X11 as in E11 (the planner refuses a launch without any display).
        if let Some(d) = std::env::var_os("DISPLAY") {
            c.env("DISPLAY", d);
        }
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(
            out.status.success(),
            "easyplay {args:?}: {}\n{}\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Copy Proton's `cmd.exe` (`bits` 64 or 32) into its own game folder and add it.
    fn add_cmd(&self, name: &str, bits: u32) -> PathBuf {
        let src = self.inputs.proton.join(match bits {
            64 => "files/lib/wine/x86_64-windows/cmd.exe",
            _ => "files/lib/wine/i386-windows/cmd.exe",
        });
        let dir = self.root.join("games").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(format!("{name}.exe"));
        std::fs::copy(&src, &exe).unwrap();
        let proton = self.inputs.proton.display().to_string();
        self.ok(&[
            "add",
            exe.to_str().unwrap(),
            "--name",
            name,
            "--preset",
            "offline",
            "--runtime",
            &proton,
            "--no-launch",
        ]);
        exe
    }

    /// The `status --json` document.
    fn status(&self) -> serde_json::Value {
        serde_json::from_str(&self.ok(&["status", "--json"])).unwrap()
    }

    /// End reason and exit status of the newest ended session.
    fn last_end(&self) -> (String, Option<i64>) {
        let s = self.status();
        let r = &s["recent"][0];
        (
            r["end_reason"].as_str().unwrap_or("-").to_owned(),
            r["exit_status"].as_i64(),
        )
    }

    /// Wait until `name` has an active session in state `state` and return its document.
    fn wait_active(&self, state: &str, timeout: Duration) -> serde_json::Value {
        let t = Instant::now();
        loop {
            let s = self.status();
            if let Some(a) = s["active"]
                .as_array()
                .and_then(|a| a.iter().find(|a| a["state"] == state))
            {
                return a.clone();
            }
            assert!(t.elapsed() < timeout, "no {state} session: {s}");
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    fn wait_idle(&self, timeout: Duration) {
        let t = Instant::now();
        while self.status()["active"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        {
            assert!(t.elapsed() < timeout, "session did not end");
            std::thread::sleep(Duration::from_millis(300));
        }
    }

    fn game_logs(&self) -> String {
        let mut text = String::new();
        let mut stack = vec![self.root.join("state")];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.file_name().is_some_and(|n| n == "game.log") {
                    text.push_str(&std::fs::read_to_string(&p).unwrap_or_default());
                }
            }
        }
        text
    }
}

/// E8: `sandbox print --format shell` produces a script that runs unmodified (the planner's
/// `--file` stand-in for `/etc/machine-id`, F-227): the prefix is created offline.
#[test]
fn e8_printed_shell_command_runs_unmodified() {
    let Some(i) = inputs() else { return };
    let lab = Lab::new(i, true);
    let exe = lab.add_cmd("CmdTest", 64);
    // The JSON game fixture of E8: arguments `/c exit 7`, Isolated runtime.
    let fixture = lab.root.join("game.json");
    std::fs::write(
        &fixture,
        serde_json::json!({
            "exe": exe,
            "args": ["/c", "exit 7"],
            "runtime": lab.inputs.proton,
            "runtime_isolation": "isolated",
        })
        .to_string(),
    )
    .unwrap();
    let script = lab.ok(&[
        "sandbox",
        "print",
        fixture.to_str().unwrap(),
        "--format",
        "shell",
    ]);
    assert!(
        script.contains("--unshare-net") || script.contains("--unshare-all"),
        "offline: {script}"
    );
    let path = lab.root.join("launch.sh");
    std::fs::write(&path, &script).unwrap();
    let out = Command::new("sh")
        .arg(&path)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(7),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let prefixes = lab.root.join("data/games");
    let found = std::fs::read_dir(&prefixes)
        .unwrap()
        .flatten()
        .any(|g| g.path().join("prefix/system.reg").exists());
    assert!(found, "no prefix created under {}", prefixes.display());
}

/// E9 (and E10's adopt): `launch --wait` records the exit status through umu, the init shim,
/// bwrap and the supervisor; `stop`, `stop --force` and `kill -9` of the supervisor end as
/// documented; the user's umu data is untouched.
#[test]
fn e9_e10_launch_status_stop_and_recovery() {
    let Some(i) = inputs() else { return };
    let before = tree_fingerprint(&i.umu_dir, 2);
    let proton_before = tree_fingerprint(&i.proton, 1);
    let lab = Lab::new(i, true);
    lab.add_cmd("CmdTest", 64);

    lab.ok(&["launch", "CmdTest", "--wait", "--", "/c", "exit 7"]);
    assert_eq!(lab.last_end(), ("exited".into(), Some(7)));

    // Graceful stop.
    lab.ok(&["launch", "CmdTest", "--", "/c", "ping -n 120 127.0.0.1"]);
    let a = lab.wait_active("running", Duration::from_secs(120));
    assert!(
        a["processes"].as_array().is_some_and(|p| !p.is_empty()),
        "{a}"
    );
    lab.ok(&["stop", "CmdTest"]);
    lab.wait_idle(Duration::from_secs(30));
    assert_eq!(lab.last_end().0, "stopped_by_user");

    // Forced stop.
    lab.ok(&["launch", "CmdTest", "--", "/c", "ping -n 120 127.0.0.1"]);
    lab.wait_active("running", Duration::from_secs(120));
    lab.ok(&["stop", "--force", "CmdTest"]);
    lab.wait_idle(Duration::from_secs(30));
    assert_eq!(lab.last_end().0, "killed");

    // The supervisor dies: nothing survives and the next command closes the row.
    lab.ok(&["launch", "CmdTest", "--", "/c", "ping -n 120 127.0.0.1"]);
    let a = lab.wait_active("running", Duration::from_secs(120));
    let sup = a["supervisor_pid"]
        .as_i64()
        .unwrap_or_else(|| panic!("no supervisor_pid in {a}"));
    Command::new("kill")
        .args(["-9", &sup.to_string()])
        .status()
        .unwrap();
    std::thread::sleep(Duration::from_secs(2));
    lab.wait_idle(Duration::from_secs(30));
    assert_eq!(lab.last_end().0, "launcher_terminated");

    assert_eq!(
        tree_fingerprint(&lab.inputs.umu_dir, 2),
        before,
        "umu data changed"
    );
    assert_eq!(
        tree_fingerprint(&lab.inputs.proton, 1),
        proton_before,
        "Proton build changed"
    );
}

/// E11: the seccomp filter is on for every process of a 32-bit session, and nothing in the
/// session needed a refused system call.
#[test]
fn e11_seccomp_with_32_bit_programs() {
    let Some(i) = inputs() else { return };
    let lab = Lab::new(i, true);
    lab.add_cmd("Cmd32", 32);
    lab.ok(&["launch", "Cmd32", "--wait", "--", "/c", "exit 5"]);
    assert_eq!(lab.last_end(), ("exited".into(), Some(5)));

    lab.ok(&["launch", "Cmd32", "--", "/c", "ping -n 40 127.0.0.1"]);
    let a = lab.wait_active("running", Duration::from_secs(120));
    let pids: Vec<i64> = a["processes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["pid"].as_i64())
        .collect();
    let mut checked = 0;
    for pid in &pids {
        let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
            continue;
        };
        let name = status.lines().next().unwrap_or_default().to_owned();
        let seccomp = status
            .lines()
            .find_map(|l| l.strip_prefix("Seccomp:"))
            .map(str::trim)
            .unwrap_or("?");
        assert_eq!(seccomp, "2", "{pid} {name} runs without the filter");
        checked += 1;
    }
    assert!(checked > 0, "no sandbox process found in {a}");
    lab.ok(&["stop", "Cmd32"]);
    lab.wait_idle(Duration::from_secs(30));
    let log = lab.game_logs();
    for bad in ["Bad system call", "EPERM", "ENOSYS"] {
        assert!(!log.contains(bad), "{bad} in game.log");
    }
}

/// E12: first run from an empty store: the launch provisions UMU-Proton and the runtime
/// through a networked Prepare, then runs the game offline. Downloads about 2 GB.
#[test]
fn e12_first_run_from_nothing() {
    let Some(i) = inputs() else { return };
    if std::env::var_os("EASYPLAY_E2E_NET").is_none_or(|v| v != "1") {
        eprintln!("skipped: EASYPLAY_E2E_NET=1 not set");
        return;
    }
    let lab = Lab::new(i, false);
    // No search directory: nothing external may be used.
    std::fs::write(lab.root.join("config/config.toml"), "").unwrap();
    let dir = lab.root.join("games/CmdTest");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("CmdTest.exe");
    std::fs::copy(
        lab.inputs
            .proton
            .join("files/lib/wine/x86_64-windows/cmd.exe"),
        &exe,
    )
    .unwrap();
    lab.ok(&[
        "add",
        exe.to_str().unwrap(),
        "--name",
        "CmdTest",
        "--no-launch",
    ]);
    let out = lab
        .cmd(&["launch", "CmdTest", "--wait", "--", "/c", "exit 7"])
        .stderr(Stdio::inherit())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(lab.last_end(), ("exited".into(), Some(7)));
    let verify = lab.ok(&["runtimes", "verify"]);
    assert!(!verify.contains("changed"), "{verify}");
}
