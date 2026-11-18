//! Sandbox security suite (SEC-002, TESTING_STRATEGY §8, THREAT_MODEL §10).
//!
//! Plans a real sandbox per preset with the real planner and the real host facts, runs
//! `probe-payload` in place of `umu-run` under the real `bwrap` (with the seccomp filter)
//! and asserts the probe table. Only with `ZAUN_E2E=1` (needs unprivileged user
//! namespaces and `bwrap`).
//!
//! Every Zaun location, the `$HOME` with its decoy secrets, the runtime and the game
//! are created under `CARGO_TARGET_TMPDIR`; the host is only read (host facts, the user's
//! runtime directory listing, `/proc/bus/input/devices`, `/etc/machine-id`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::os::fd::OwnedFd;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::{SocketAddr, UnixListener};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use zaun_core::clock::{SystemClock, Timestamp};
use zaun_core::fsx::{MountTable, RealFs};
use zaun_core::host::{
    HostFacts, NoCompositor, ProbeDeps, RealAccess, RealRunner, UmuFacts, probe_all,
};
use zaun_core::ids::{GameId, RuntimeName, SessionId};
use zaun_core::launch::{PlanFd, PlanSandbox};
use zaun_core::model::{
    CompatOptions, FolderAccess, FolderGrant, GameDirAccess, GrantOrigin, ResolvedRuntime,
    RuntimeIsolationMode, RuntimeOrigin, SandboxPolicy, SessionKind, SlrVariant, WorkingDir,
};
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_core::sandbox::{self, GameContext, GamePaths, LaunchInputs, SandboxContext, SessionDirs};
use zaun_core::session::spawn::{HostActionCtx, run_host_actions};
use zaun_core::session::{PrefixLock, RealSpawner, SandboxIo, SandboxSpawner};
use zaun_testkit::bins::workspace_bin;

fn enabled() -> bool {
    std::env::var_os("ZAUN_E2E").is_some_and(|v| v == "1")
}

/// One probe result.
#[derive(Clone, Debug)]
struct Probe {
    verdict: String,
    detail: String,
}

type Results = BTreeMap<String, Probe>;

/// The fixture: throw-away roots, decoys, a fake runtime, a game and the host facts.
struct World {
    _root: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    paths: AppPaths,
    host: HostFacts,
    mounts: MountTable,
    fs: RealFs,
    bin: PathBuf,
    init: PathBuf,
    umu: UmuFacts,
    runtime: ResolvedRuntime,
    game: GameId,
    game_paths: GamePaths,
    game_root: PathBuf,
    exe: PathBuf,
    other_prefix: PathBuf,
    ro: PathBuf,
    rw: PathBuf,
    user: String,
    group: String,
    host_env: BTreeMap<String, String>,
}

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn copy_exe(from: &Path, to: &Path) {
    std::fs::copy(from, to).unwrap();
}

fn group_name(gid: u32) -> String {
    std::fs::read_to_string("/etc/group")
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            (f.len() > 2 && f[2] == gid.to_string()).then(|| f[0].to_owned())
        })
        .unwrap_or_else(|| "users".into())
}

impl World {
    fn new() -> Self {
        let tmp = tempfile::Builder::new()
            .prefix("e2e-sandbox-")
            .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
            .unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("home");
        for d in ["home", "config", "data", "cache", "state", "run", "bin"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // Decoys (THREAT_MODEL T-01…T-05).
        write(&home.join(".ssh/id_ed25519"), "DECOY PRIVATE KEY\n");
        write(&home.join("Documents/secret.txt"), "DECOY SECRET\n");
        write(&home.join(".mozilla/firefox/profiles.ini"), "[Profile0]\n");
        write(
            &home.join(".config/google-chrome/Default/Cookies"),
            "DECOY\n",
        );
        write(&home.join(".bashrc"), "# original\n");
        std::fs::create_dir_all(home.join(".local/bin")).unwrap();

        let uid = rustix::process::getuid().as_raw();
        let gid = rustix::process::getgid().as_raw();
        let mut env = PathEnv::new(uid);
        for (k, v) in [
            ("HOME", home.clone()),
            ("XDG_RUNTIME_DIR", root.join("run")),
            ("ZAUN_CONFIG_DIR", root.join("config")),
            ("ZAUN_DATA_DIR", root.join("data")),
            ("ZAUN_CACHE_DIR", root.join("cache")),
            ("ZAUN_STATE_DIR", root.join("state")),
        ] {
            env.set(k, v);
        }
        let paths = AppPaths::resolve(&env).unwrap();
        write(&paths.db_file, "DECOY DATABASE\n");
        write(&paths.config_file, "# decoy config\n");
        write(
            &paths.runtime_manifests_dir.join("steamrt3.json"),
            "{\"decoy\":true}\n",
        );

        // Host facts of the real machine; Zaun's own locations stay throw-away.
        let clock = Arc::new(SystemClock::new());
        let real_env = PathEnv::from_process();
        let user = real_env
            .get("USER")
            .and_then(|u| u.to_str())
            .unwrap_or("user")
            .to_owned();
        let host = probe_all(&ProbeDeps {
            fs: Arc::new(RealFs::new()),
            runner: Arc::new(RealRunner::new(clock.clone())),
            access: Arc::new(RealAccess),
            clock: clock.clone(),
            env: real_env,
            paths: paths.clone(),
            umu_override: None,
            user_name: Some(user.clone()),
            jobs: None,
            // The restricted Wayland socket has its own test below.
            compositor: Arc::new(NoCompositor),
        });
        assert!(host.bwrap.is_some(), "bwrap not found: {:?}", host.problems);

        // Binaries, copied out of the shared target directory.
        let bin = root.join("bin");
        let init = bin.join("zaun-init");
        copy_exe(&workspace_bin("zaun-init"), &init);
        copy_exe(&workspace_bin("probe-payload"), &bin.join("probe-payload"));
        let umu = UmuFacts {
            path: bin.join("probe-payload"),
            version: None,
            closure: vec![bin.clone()],
            wrapper: None,
        };

        // A Proton build and an SLR-shaped tree in the store (never executed).
        let proton = paths.compat_tools_dir.join("TestProton");
        write(&proton.join("proton"), "#!/bin/sh\nexit 99\n");
        let slr = paths.slr_dir(SlrVariant::Steamrt3);
        let platform = "sniper_platform_0.1";
        for f in [
            "toolmanifest.vdf",
            "VERSIONS.txt",
            "_v2-entry-point",
            "run",
            "mtree.txt.gz",
        ] {
            write(&slr.join(f), "x\n");
        }
        write(&slr.join(".installed.ok"), "ok\n");
        write(&slr.join(platform).join("files/.ref"), "");
        std::fs::create_dir_all(slr.join("pressure-vessel/bin")).unwrap();
        std::os::unix::fs::symlink("_v2-entry-point", slr.join("umu")).unwrap();
        let runtime = ResolvedRuntime {
            name: RuntimeName::new("TestProton").unwrap(),
            proton_path_host: proton.clone(),
            proton_path_sandbox: proton,
            origin: RuntimeOrigin::Store,
            slr_variant: SlrVariant::Steamrt3,
            slr_dir: slr,
            platform_dir: platform.into(),
            isolation: RuntimeIsolationMode::Isolated,
        };

        // The game, another game's prefix and two granted folders.
        let game = GameId::generate(&*clock);
        let game_paths = GamePaths::new(&paths, game);
        let game_root = root.join("games/Test Game");
        let exe = game_root.join("game.exe");
        write(&exe, "MZ original\n");
        let other_prefix = paths.prefix_dir(GameId::generate(&*clock));
        write(&other_prefix.join("marker.txt"), "other game\n");
        let ro = root.join("grants/ro");
        let rw = root.join("grants/rw");
        write(&ro.join("granted.txt"), "granted\n");
        std::fs::create_dir_all(&rw).unwrap();

        let mut host_env: BTreeMap<String, String> = std::env::vars().collect();
        host_env.insert("AWS_SECRET_ACCESS_KEY".into(), "decoy".into());
        host_env.insert("HOME".into(), home.display().to_string());

        let fs = RealFs::new();
        let mounts = MountTable::read(&fs).unwrap();
        World {
            _root: tmp,
            root,
            home,
            paths,
            host,
            mounts,
            fs,
            bin,
            init,
            umu,
            runtime,
            game,
            game_paths,
            game_root,
            exe,
            other_prefix,
            ro,
            rw,
            user,
            group: group_name(gid),
            host_env,
        }
    }

    /// The preset with the grants of P-23/P-24 and a read-only game folder (P-26).
    fn policy(&self, preset: &str) -> SandboxPolicy {
        let mut p = match preset {
            "offline" => sandbox::offline(),
            "online" => sandbox::online(),
            _ => sandbox::standard(&self.host),
        };
        p.game_dir = GameDirAccess::ReadOnly;
        for (dir, access) in [
            (&self.ro, FolderAccess::ReadOnly),
            (&self.rw, FolderAccess::ReadWrite),
        ] {
            p.folders.push(FolderGrant {
                path: dir.clone(),
                canonical: dir.clone(),
                access,
                origin: GrantOrigin::UserChosen,
                drive_letter: None,
                granted_at: Timestamp::EPOCH,
            });
        }
        p
    }

    /// Plan and render one sandbox running `probe-payload <args>`.
    fn plan(&self, policy: &SandboxPolicy, args: &[String]) -> (PlanSandbox, SessionId) {
        let sid = SessionId::generate(&SystemClock::new());
        let session = SessionDirs::new(&self.paths, sid);
        let umu_args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let wd = WorkingDir::ExeDir;
        let compat = CompatOptions::default();
        let uid = rustix::process::getuid().as_raw();
        let ctx = SandboxContext {
            kind: SessionKind::Game,
            uid,
            gid: rustix::process::getgid().as_raw(),
            user_name: &self.user,
            group_name: &self.group,
            home: &self.home,
            game: Some(GameContext {
                id: self.game,
                private_home: &self.game_paths.private_home,
                prefix: &self.game_paths.prefix,
                overlay_upper: &self.game_paths.overlay_upper,
                overlay_work: &self.game_paths.overlay_work,
                root: Some(&self.game_root),
                exe: &self.exe,
                umu_id: None,
                store: None,
                prefix_lock: &self.game_paths.prefix_lock,
            }),
            runtime: Some(&self.runtime),
            proton_codename: None,
            session: &session,
            paths: &self.paths,
            init_binary: &self.init,
            umu: &self.umu,
            network_override: None,
            launch: LaunchInputs {
                working_dir: &wd,
                compat: &compat,
                global_env: &[],
                game_env: &[],
                umu_args: &umu_args,
                verbose_log: false,
            },
            host_env: &self.host_env,
            install_id: "0123456789abcdef0123456789abcdef",
            developer_mode: false,
            seccomp_available: sandbox::seccomp::AVAILABLE,
            user_dirs: &[],
            other_game_roots: &[],
            mounts: &self.mounts,
            fs: &self.fs,
        };
        let plan = sandbox::plan(policy, &ctx, &self.host)
            .unwrap_or_else(|issues| panic!("planning failed: {issues:#?}"));
        let mut locks: Vec<PrefixLock> = Vec::new();
        run_host_actions(
            &plan.host_actions,
            &mut HostActionCtx {
                fs: &self.fs,
                uid,
                locks: &mut locks,
                mark_tainted: &mut |_| Ok(()),
            },
        )
        .unwrap();
        let rendered = sandbox::render(&plan).unwrap();
        (
            PlanSandbox {
                args: rendered.args.clone(),
                fds: rendered.fds.iter().map(PlanFd::from_payload).collect(),
                command: rendered.command.clone(),
            },
            sid,
        )
    }

    fn bwrap(&self) -> PathBuf {
        self.host.bwrap.as_ref().unwrap().path.clone()
    }
}

/// A running sandbox: its output lines arrive on `lines`.
struct Run {
    child: Box<dyn zaun_core::session::SandboxChild>,
    lines: mpsc::Receiver<String>,
    _keep: Vec<OwnedFd>,
}

fn pipe() -> (OwnedFd, OwnedFd) {
    let (r, w) = std::io::pipe().unwrap();
    (r.into(), w.into())
}

fn start(w: &World, sandbox: &PlanSandbox) -> Run {
    let (sync_r, sync_w) = pipe();
    start_with_sync(w, sandbox, sync_r, sync_w)
}

/// [`start`] with the `--sync-fd` pipe made by the caller.
fn start_with_sync(w: &World, sandbox: &PlanSandbox, sync_r: OwnedFd, sync_w: OwnedFd) -> Run {
    let (log_r, log_w) = pipe();
    let (st_r, st_w) = pipe();
    let (init_r, init_w) = pipe();
    let child = RealSpawner
        .spawn(
            &w.bwrap(),
            sandbox,
            SandboxIo {
                log: log_w,
                status: st_w,
                init_status: init_w,
                sync: sync_w,
            },
        )
        .unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(std::fs::File::from(log_r)).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Run {
        child,
        lines: rx,
        _keep: vec![st_r, init_r, sync_r],
    }
}

/// Run to the end and collect the probe lines (and the whole log for messages).
fn run(w: &World, sandbox: &PlanSandbox) -> (Results, String) {
    let mut r = start(w, sandbox);
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut log = String::new();
    loop {
        match r.lines.recv_timeout(Duration::from_millis(100)) {
            Ok(l) => {
                log.push_str(&l);
                log.push('\n');
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if Instant::now() > deadline {
            let _ = r.child.kill_monitor();
            panic!("sandbox did not finish:\n{log}");
        }
    }
    while r.child.try_wait().unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(20));
    }
    (parse(&log), log)
}

fn parse(log: &str) -> Results {
    log.lines()
        .filter_map(|l| l.strip_prefix("PROBE "))
        .filter_map(|l| {
            let mut it = l.splitn(3, ' ');
            Some((
                it.next()?.to_owned(),
                Probe {
                    verdict: it.next()?.to_owned(),
                    detail: it.next().unwrap_or("").to_owned(),
                },
            ))
        })
        .collect()
}

/// Sockets in the user's real runtime directory that no policy binds (P-12), read-only.
fn host_agent_sockets(host: &HostFacts) -> Vec<String> {
    let Some(xrd) = host.display.xdg_runtime_dir.clone() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut stack = vec![(xrd.clone(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.filter_map(Result::ok) {
            let Ok(ft) = e.file_type() else { continue };
            let rel = e.path().strip_prefix(&xrd).unwrap().display().to_string();
            if ft.is_dir() && depth < 2 && !rel.starts_with("zaun") {
                stack.push((e.path(), depth + 1));
            } else if ft.is_socket()
                && !rel.starts_with("wayland-")
                && !rel.starts_with("pipewire-")
                && rel != "pulse/native"
                && rel != "bus"
            {
                out.push(e.path().display().to_string());
            }
        }
    }
    out.sort();
    out.truncate(20);
    out
}

/// Keyboard-class event nodes of the host (P-17), from `/proc/bus/input/devices`.
fn host_keyboards() -> Vec<String> {
    let text = std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default();
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let handlers = block
            .lines()
            .find_map(|l| l.strip_prefix("H: Handlers="))
            .unwrap_or("");
        let ev = block
            .lines()
            .find_map(|l| l.strip_prefix("B: EV="))
            .and_then(|v| u64::from_str_radix(v.trim(), 16).ok())
            .unwrap_or(0);
        if handlers.split_whitespace().any(|h| h == "kbd") && ev & 0x12_0000 == 0x12_0000 {
            out.extend(
                handlers
                    .split_whitespace()
                    .filter(|h| h.starts_with("event"))
                    .map(|h| format!("/dev/input/{h}")),
            );
        }
    }
    out
}

fn host_block_devices() -> Vec<String> {
    std::fs::read_dir("/sys/block")
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| {
                    !n.starts_with("loop") && !n.starts_with("ram") && !n.starts_with("zram")
                })
                .map(|n| format!("/dev/{n}"))
                .collect()
        })
        .unwrap_or_default()
}

/// The probe arguments with live host listeners (kept alive by the caller).
struct Listeners {
    _tcp: TcpListener,
    _abs: UnixListener,
    args: Vec<String>,
}

fn listeners(w: &World) -> Listeners {
    // Unique per call: `cargo test` runs these tests as threads of one process.
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = format!("zaun-e2e-{}-{n}", std::process::id());
    let abs =
        UnixListener::bind_addr(&SocketAddr::from_abstract_name(name.as_bytes()).unwrap()).unwrap();
    let mut args = vec![
        format!("home={}", w.home.display()),
        format!("data={}", w.paths.data_dir.display()),
        format!("config={}", w.paths.config_file.display()),
        format!("other_prefix={}", w.other_prefix.display()),
        format!("proton={}", w.runtime.proton_path_host.display()),
        // The runtime appears where umu expects it, in the sandbox's home.
        format!("slr={}", w.home.join(".local/share/umu/steamrt3").display()),
        format!("tcp={}", tcp.local_addr().unwrap()),
        format!("abstract={name}"),
        format!("host_pid={}", std::process::id()),
        "runner=e2e_sandbox".to_owned(),
        format!("ro={}", w.ro.display()),
        format!("rw={}", w.rw.display()),
        format!("exe={}", w.exe.display()),
    ];
    let bus = w
        .host
        .display
        .xdg_runtime_dir
        .as_ref()
        .map_or_else(|| PathBuf::from("/run/user/0/bus"), |x| x.join("bus"));
    args.push(format!("bus={}", bus.display()));
    args.extend(
        host_agent_sockets(&w.host)
            .into_iter()
            .map(|s| format!("sock={s}")),
    );
    args.extend(host_keyboards().into_iter().map(|k| format!("kbd={k}")));
    args.extend(
        host_block_devices()
            .into_iter()
            .map(|b| format!("block={b}")),
    );
    Listeners {
        _tcp: tcp,
        _abs: abs,
        args,
    }
}

fn expect(r: &Results, id: &str, verdict: &str, log: &str) {
    let p = r.get(id).unwrap_or_else(|| panic!("{id} missing:\n{log}"));
    assert_eq!(p.verdict, verdict, "{id}: {}\n{log}", p.detail);
}

fn host_machine_id() -> String {
    std::fs::read_to_string("/etc/machine-id")
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn check_host_untouched(w: &World) {
    assert_eq!(
        std::fs::read_to_string(w.home.join(".bashrc")).unwrap(),
        "# original\n"
    );
    assert!(!w.home.join(".config/autostart").exists());
    assert!(!w.home.join(".local/bin/x").exists());
    assert!(!w.other_prefix.join("probe.txt").exists());
    assert!(!w.runtime.proton_path_host.join("probe.txt").exists());
    assert!(!w.runtime.slr_dir.join("probe.txt").exists());
    assert!(!w.ro.join("probe.txt").exists());
    assert_eq!(std::fs::read_to_string(&w.exe).unwrap(), "MZ original\n");
    assert!(!w.exe.with_extension("new").exists());
}

fn assert_preset(w: &World, preset: &str, r: &Results, log: &str) {
    let seccomp = &r.get("P-00").unwrap().detail;
    assert!(
        seccomp.contains("seccomp=2"),
        "{preset}: filter not applied: {seccomp}"
    );
    for id in [
        "P-01", "P-02", "P-03", "P-05", "P-06", "P-07", "P-11", "P-13", "P-14", "P-16",
    ] {
        expect(r, id, "DENIED", log);
    }
    // The runtime and Proton are visible and read-only (Isolated mode).
    let p07 = &r.get("P-07").unwrap().detail;
    assert!(
        p07.contains("slr_modify=EROFS") && p07.contains("proton_modify=EROFS"),
        "{p07}"
    );
    if r.contains_key("P-12") {
        expect(r, "P-12", "DENIED", log);
    }
    expect(r, "P-17", "DENIED", log);
    let net = if preset == "offline" {
        "DENIED"
    } else {
        "ALLOWED"
    };
    expect(r, "P-09", net, log);
    expect(r, "P-10", net, log);
    let mid = r.get("P-14").unwrap().detail.clone();
    let host_mid = host_machine_id();
    if preset == "standard" {
        assert!(mid.contains(&format!("machine_id={host_mid}")), "{mid}");
    } else {
        assert!(!host_mid.is_empty() && !mid.contains(&host_mid), "{mid}");
    }
    assert_eq!(r.get("P-15").unwrap().detail, "hostname=zaun");
    let p19 = if preset == "standard" {
        "ALLOWED"
    } else {
        "DENIED"
    };
    expect(r, "P-19", p19, log);
    expect(r, "P-20", "DENIED", log);
    let p20 = &r.get("P-20").unwrap().detail;
    assert!(p20.contains("keyctl_i386=EPERM"), "{p20}");
    assert!(p20.contains("getpid_x32=ENOSYS"), "{p20}");
    expect(r, "P-21", "DENIED", log);
    assert!(r.get("P-21").unwrap().detail.contains("fd0=EPERM"));
    expect(r, "P-22", "ALLOWED", log);
    expect(r, "P-23R", "ALLOWED", log);
    expect(r, "P-23W", "DENIED", log);
    expect(r, "P-24W", "ALLOWED", log);
    expect(r, "P-24L", "DENIED", log);
    expect(r, "P-26", "DENIED", log);
    check_host_untouched(w);
}

#[test]
fn e2e_probe_table_per_preset() {
    if !enabled() {
        return;
    }
    let w = World::new();
    let l = listeners(&w);
    for preset in ["offline", "online", "standard"] {
        let (sb, _) = w.plan(&w.policy(preset), &l.args);
        assert!(
            sb.args.iter().any(|a| a == "--seccomp"),
            "{preset}: no --seccomp in the plan"
        );
        let (r, log) = run(&w, &sb);
        eprintln!("== {preset}\n{log}");
        assert_preset(&w, preset, &r, &log);
    }
    let _ = &w.root;
}

/// A deliberately weakened plan (test-only edits of the rendered options) makes the
/// expected probes fail: the suite detects regressions instead of passing vacuously.
#[test]
fn e2e_weakened_plans_are_detected() {
    if !enabled() {
        return;
    }
    let w = World::new();
    let l = listeners(&w);
    let (mut sb, _) = w.plan(&w.policy("offline"), &l.args);
    // Drop the filter.
    let i = sb.args.iter().position(|a| a == "--seccomp").unwrap();
    sb.args.drain(i..i + 2);
    sb.fds.retain(|f| f.fd != sandbox::plan::FD_SECCOMP);
    // Expose the real `.ssh` decoy.
    let ssh = w.home.join(".ssh");
    sb.args
        .extend([OsString::from("--ro-bind"), ssh.clone().into(), ssh.into()]);
    let (r, log) = run(&w, &sb);
    expect(&r, "P-01", "ALLOWED", &log);
    expect(&r, "P-20", "ALLOWED", &log);
    assert!(r.get("P-00").unwrap().detail.contains("seccomp=0"), "{log}");
    expect(&r, "P-22", "ALLOWED", &log);
}

/// P-18: a double-forked daemon does not outlive the sandbox.
#[test]
fn e2e_daemon_does_not_survive_the_sandbox() {
    if !enabled() {
        return;
    }
    let w = World::new();
    let token = format!("epd{}", std::process::id() % 100_000);
    let (sb, _) = w.plan(&w.policy("offline"), &[format!("daemon={token}")]);
    let mut r = start(&w, &sb);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let l = r.lines.recv_timeout(Duration::from_secs(30)).unwrap();
        if l.starts_with("PROBE P-18") {
            break;
        }
        assert!(Instant::now() < deadline);
    }
    let alive = |t: &str| {
        std::fs::read_dir("/proc")
            .unwrap()
            .filter_map(Result::ok)
            .any(|e| std::fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim() == t))
    };
    // The session is still running: the init shim waits for the daemon.
    std::thread::sleep(Duration::from_millis(300));
    assert!(alive(&token), "daemon not started");
    assert!(
        r.child.try_wait().unwrap().is_none(),
        "sandbox ended with a live daemon"
    );
    r.child.kill_monitor().unwrap();
    while r.child.try_wait().unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(20));
    }
    let gone = Instant::now() + Duration::from_secs(5);
    while alive(&token) && Instant::now() < gone {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!alive(&token), "daemon survived the sandbox");
}

/// The filter itself is accepted by the kernel through `bwrap --seccomp` on a minimal
/// sandbox (no planner involved), for both profiles.
#[test]
fn e2e_filter_loads_in_bwrap() {
    if !enabled() {
        return;
    }
    let w = World::new();
    for net in [false, true] {
        let mut args: Vec<OsString> = [
            "--unshare-user",
            "--unshare-pid",
            "--die-with-parent",
            "--new-session",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--json-status-fd",
            "4",
            "--sync-fd",
            "5",
            "--seccomp",
            "7",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        if net {
            args.push("--unshare-net".into());
        }
        let sb = PlanSandbox {
            args,
            fds: vec![PlanFd {
                fd: 7,
                target: None,
                kind: zaun_core::launch::FdKind::Seccomp,
                content_b64: String::new(),
            }],
            command: vec![w.bin.join("probe-payload").into(), "only=P-20,P-22".into()],
        };
        let (r, log) = run(&w, &sb);
        assert!(r.get("P-00").unwrap().detail.contains("seccomp=2"), "{log}");
        expect(&r, "P-20", "DENIED", &log);
        expect(&r, "P-22", "ALLOWED", &log);
    }
}

/// ADR-0029 on the real compositor and the real `bwrap`: a restricted socket offers the
/// compositor's globals minus those it withholds from sandboxed clients, and the compositor
/// serves it exactly as long as the sandbox holds the `--sync-fd` pipe it was tied to.
/// Skipped without a compositor that offers security contexts.
#[test]
fn e2e_wayland_context_lives_as_long_as_the_sandbox() {
    use std::os::fd::AsFd;
    use zaun_core::host::wayland::{
        ContextRequest, SECURITY_CONTEXT_MANAGER, create_security_context, globals,
    };
    use zaun_core::host::{CompositorProbe, LiveCompositor};

    if !enabled() {
        return;
    }
    let w = World::new();
    let Some(display) = w.host.display.wayland.as_ref().map(|s| s.path.clone()) else {
        eprintln!("skipped: no Wayland compositor");
        return;
    };
    if !LiveCompositor.offers_security_context(&display) {
        eprintln!("skipped: the compositor offers no security contexts");
        return;
    }
    // A short directory: socket addresses hold 107 bytes.
    let dir = tempfile::Builder::new()
        .prefix("zn-wl")
        .tempdir_in("/tmp")
        .unwrap();
    let socket = dir.path().join("wayland");
    let (sync_r, sync_w) = pipe();
    create_security_context(
        &ContextRequest {
            display: &display,
            socket: &socket,
            app_id: "e2e",
            instance_id: "1",
        },
        sync_r.as_fd(),
    )
    .unwrap();

    let raw = globals(&display).unwrap();
    let restricted = globals(&socket).unwrap();
    let withheld: Vec<&String> = raw.iter().filter(|g| !restricted.contains(g)).collect();
    eprintln!("withheld from the restricted socket: {withheld:?}");
    assert!(
        restricted.iter().all(|g| raw.contains(g)),
        "the restricted socket offers more: {restricted:?}"
    );
    assert!(
        !restricted.iter().any(|g| g == SECURITY_CONTEXT_MANAGER),
        "a sandboxed client could nest contexts"
    );

    // The sandbox holds the only write end: served while it runs ...
    let sleep = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join("sleep"))
        .find(|p| p.is_file())
        .expect("sleep on PATH");
    let sb = PlanSandbox {
        args: [
            "--unshare-user",
            "--unshare-pid",
            "--die-with-parent",
            "--new-session",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--json-status-fd",
            "4",
            "--sync-fd",
            "5",
        ]
        .iter()
        .map(OsString::from)
        .collect(),
        fds: Vec::new(),
        command: vec![sleep.into(), "2".into()],
    };
    let mut r = start_with_sync(&w, &sb, sync_r, sync_w);
    std::thread::sleep(Duration::from_millis(300));
    assert!(r.child.try_wait().unwrap().is_none(), "sandbox ended early");
    assert!(
        globals(&socket).is_ok(),
        "not served while the sandbox runs"
    );

    // ... and no longer once it is gone.
    let deadline = Instant::now() + Duration::from_secs(10);
    while r.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "sandbox did not end");
        std::thread::sleep(Duration::from_millis(20));
    }
    let gone = Instant::now() + Duration::from_secs(3);
    while globals(&socket).is_ok() {
        assert!(
            Instant::now() < gone,
            "still served after the sandbox ended"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
