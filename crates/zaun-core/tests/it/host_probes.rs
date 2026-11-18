//! Host probes against fixture trees and scripted commands (HOST-001 … HOST-008).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use zaun_core::error::ErrorCode;
use zaun_core::fsx::Fs;
use zaun_core::host::bwrap_probe::{
    BwrapProbeInput, nested_args, overlay_args, probe_bwrap, userns_args,
};
use zaun_core::host::etc_links::probe_etc_links;
use zaun_core::host::input::probe_input;
use zaun_core::host::layout::{LayoutInput, probe_layout};
use zaun_core::host::probe::{CacheKey, load_cache, revalidate, save_cache};
use zaun_core::host::umu_probe::{LocateError, LocateInput, locate, probe_umu};
use zaun_core::host::{
    BwrapFlag, HostEntry, HostFacts, HostHint, HostProbe, InputClass, KernelFacts, LayoutKind,
    LiveHostProbe, ModeAccess, NoCompositor, OsRelease, ProbeDeps, ResolverStrategy, ToolVersion,
    probe_all,
};
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_testkit::fixtures::host_trees::{self, INPUT_GID, USER_UID};
use zaun_testkit::{CannedOutput, FakeClock, MemFs, ScriptedRunner};

const HELP_0_12: &str = include_str!("../data/bwrap/help-0.12.0.txt");
const HELP_0_9: &str = include_str!("../data/bwrap/help-0.9.0.txt");

fn layout_of(fs: &MemFs, user: Option<&str>) -> zaun_core::host::FsLayout {
    probe_layout(
        fs,
        &LayoutInput {
            os: None,
            user_name: user,
        },
    )
}

#[test]
fn arch_layout() {
    let fs = MemFs::new();
    host_trees::arch(&fs);
    let l = layout_of(&fs, None);
    assert_eq!(l.kind, LayoutKind::Fhs);
    assert!(l.is_merged("/bin") && l.is_merged("/lib") && l.is_merged("/lib64"));
    assert_eq!(
        l.top_level.get(Path::new("/bin")).unwrap().link_target(),
        Some(Path::new("usr/bin"))
    );
    assert!(!l.top_level.contains_key(Path::new("/lib32")));
    assert!(l.has_usr && l.has_opt);
    assert!(l.extra_lib_roots.is_empty(), "{:?}", l.extra_lib_roots);
    assert_eq!(
        l.resolver,
        ResolverStrategy::Symlink {
            target: "../run/systemd/resolve/stub-resolv.conf".into(),
            bind_dir: "/run/systemd/resolve".into()
        }
    );
    assert!(matches!(
        l.etc.get("localtime"),
        Some(HostEntry::Symlink { .. })
    ));
    assert_eq!(l.etc.get("hosts"), Some(&HostEntry::File));
    assert_eq!(l.etc.get("fonts"), Some(&HostEntry::Dir));
    assert!(!l.etc.contains_key("pki"));
    assert_eq!(l.path_env, "/usr/local/bin:/usr/bin:/bin");
    assert!(l.passthrough_env.is_empty() && l.system_trees.is_empty());
}

#[test]
fn debian_layout() {
    let fs = MemFs::new();
    host_trees::debian(&fs);
    let l = layout_of(&fs, None);
    assert_eq!(l.kind, LayoutKind::Fhs);
    for top in ["/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32"] {
        assert!(l.is_merged(top), "{top}");
    }
    assert!(l.extra_lib_roots.is_empty());
    assert_eq!(l.resolver, ResolverStrategy::File);
    assert_eq!(l.etc.get("alternatives"), Some(&HostEntry::Dir));
    assert_eq!(l.etc.get("timezone"), Some(&HostEntry::File));
    assert!(!l.has_opt);
}

#[test]
fn fedora_layout_has_extra_library_root() {
    let fs = MemFs::new();
    host_trees::fedora(&fs);
    let l = layout_of(&fs, None);
    assert_eq!(l.kind, LayoutKind::Fhs);
    // /opt and /usr entries are covered by system binds; missing ones are dropped.
    assert_eq!(l.extra_lib_roots, vec![PathBuf::from("/srv/gpu/lib64")]);
    assert_eq!(
        l.resolver,
        ResolverStrategy::Symlink {
            target: "../run/NetworkManager/resolv.conf".into(),
            bind_dir: "/run/NetworkManager".into()
        }
    );
    assert_eq!(l.etc.get("pki"), Some(&HostEntry::Dir));
    assert!(l.system_roots().contains(&PathBuf::from("/srv/gpu/lib64")));
}

#[test]
fn nixos_layout_and_etc_links() {
    let fs = MemFs::new();
    host_trees::nixos(&fs);
    let l = layout_of(&fs, Some("sam"));
    assert_eq!(l.kind, LayoutKind::Nix);
    assert!(!l.has_usr || l.top_level.get(Path::new("/bin")) == Some(&HostEntry::Dir));
    assert_eq!(l.top_level.get(Path::new("/bin")), Some(&HostEntry::Dir));
    assert!(l.passthrough_env.iter().any(|v| v == "LD_LIBRARY_PATH"));
    assert_eq!(l.path_env, "/run/current-system/sw/bin:/usr/bin:/bin");
    assert!(
        l.system_trees
            .contains_key(Path::new("/run/current-system"))
    );
    assert!(l.system_trees.contains_key(Path::new("/run/opengl-driver")));
    assert!(l.system_trees.contains_key(Path::new("/etc/static")));
    assert!(
        l.system_trees
            .contains_key(Path::new("/etc/profiles/per-user/sam"))
    );
    assert!(
        !l.system_trees
            .contains_key(Path::new("/run/opengl-driver-32"))
    );
    assert!(matches!(l.resolver, ResolverStrategy::Symlink { .. }));
    assert!(l.system_roots().contains(&PathBuf::from("/nix")));

    let links = probe_etc_links(&fs).links;
    let zi = &links["zoneinfo"];
    assert!(zi.via_etc_static && zi.is_dir);
    assert_eq!(
        zi.resolved.as_deref(),
        Some(Path::new("/nix/store/ffff-tzdata/share/zoneinfo"))
    );
    assert_eq!(zi.chain[0], Path::new("/etc/zoneinfo"));
    let ns = &links["nsswitch.conf"];
    assert!(ns.via_etc_static && !ns.is_dir);
    assert_eq!(
        ns.resolved.as_deref(),
        Some(Path::new("/nix/store/bbbb-etc/etc/nsswitch.conf"))
    );
    // localtime goes through /etc/zoneinfo, itself through /etc/static.
    assert!(links["localtime"].via_etc_static);
    assert!(
        !links.contains_key("passwd"),
        "a regular file is not a link"
    );
}

#[test]
fn input_classification() {
    let fs = Arc::new(MemFs::new());
    host_trees::input_devices(&fs);
    let user = ModeAccess::new(fs.clone(), USER_UID, vec![1000]);
    let f = probe_input(&*fs, &user);
    let get = |p: &str| f.nodes.iter().find(|n| n.path == Path::new(p)).unwrap();
    assert!(
        get("/dev/input/event3")
            .classes
            .contains(&InputClass::Keyboard)
    );
    assert_eq!(
        get("/dev/input/event4")
            .classes
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![InputClass::Key]
    );
    assert!(
        get("/dev/input/event5")
            .classes
            .contains(&InputClass::Mouse)
    );
    let xbox = get("/dev/input/event21");
    assert!(xbox.is_joystick() && xbox.readable);
    assert_eq!(xbox.siblings, vec![PathBuf::from("/dev/input/js0")]);
    assert!(get("/dev/input/js1").is_joystick());
    let hid = get("/dev/hidraw4");
    assert!(hid.is_joystick() && hid.classes.contains(&InputClass::Touchpad));
    assert!(!get("/dev/input/event23").readable);
    assert!(!f.keyboards_readable);
    assert!(f.udev_db);
    let pads: Vec<_> = f.joysticks().map(|n| n.path.clone()).collect();
    assert_eq!(pads.len(), 5, "{pads:?}");

    // input_group_user: every node is readable, so keyboards are exposed.
    let grouped = ModeAccess::new(fs.clone(), USER_UID, vec![1000, INPUT_GID]);
    assert!(probe_input(&*fs, &grouped).keyboards_readable);
}

fn locate_input<'a>(
    roots: &'a [PathBuf],
    path: Option<&'a std::ffi::OsStr>,
    dirs: &'a [PathBuf],
) -> LocateInput<'a> {
    LocateInput {
        preference: None,
        host_path: path,
        system_roots: roots,
        home: Some(Path::new("/home/sam")),
        zaun_dirs: dirs,
    }
}

fn fhs_roots() -> Vec<PathBuf> {
    zaun_core::host::FsLayout::default().system_roots()
}

#[test]
fn umu_system_package() {
    let fs = MemFs::new();
    host_trees::umu_system(&fs);
    let roots = fhs_roots();
    let l = locate(&fs, &locate_input(&roots, None, &[])).unwrap();
    assert_eq!(l.path, Path::new("/usr/bin/umu-run"));
    assert!(l.closure.is_empty() && l.wrapper.is_none());
}

#[test]
fn umu_pipx_venv() {
    let fs = MemFs::new();
    host_trees::umu_pipx(&fs, "/home/sam");
    let roots = fhs_roots();
    let path = std::ffi::OsString::from("/home/sam/.local/bin:/usr/bin");
    let l = locate(&fs, &locate_input(&roots, Some(&path), &[])).unwrap();
    assert_eq!(l.path, Path::new("/home/sam/.local/bin/umu-run"));
    assert_eq!(
        l.closure,
        vec![PathBuf::from(
            "/home/sam/.local/share/pipx/venvs/umu-launcher"
        )]
    );
}

#[test]
fn umu_uv_tool() {
    let fs = MemFs::new();
    host_trees::umu_uv(&fs, "/home/sam");
    let roots = fhs_roots();
    let path = std::ffi::OsString::from("/home/sam/.local/bin");
    let l = locate(&fs, &locate_input(&roots, Some(&path), &[])).unwrap();
    assert_eq!(
        l.closure,
        vec![
            PathBuf::from("/home/sam/.local/share/uv/python/cpython-3.13.1-linux-x86_64-gnu"),
            PathBuf::from("/home/sam/.local/share/uv/tools/umu-launcher"),
        ]
    );
}

#[test]
fn umu_nix_wrapper_chain() {
    let fs = MemFs::new();
    host_trees::nixos(&fs);
    host_trees::umu_nix_wrapper(&fs);
    let layout = layout_of(&fs, None);
    let roots = layout.system_roots();
    let l = locate(&fs, &locate_input(&roots, None, &[])).unwrap();
    assert_eq!(l.path, Path::new("/run/current-system/sw/bin/umu-run"));
    assert!(l.closure.is_empty());
    let w = l.wrapper.unwrap();
    assert!(w.sandboxed && !w.reaches_python);
    // nixpkgs' buildFHSEnv wrapper works nested (TECHNICAL_RESEARCH F-228): no warning.
    assert!(w.fhs_env && !w.breaks_nesting());
    assert_eq!(
        w.chain,
        vec![
            PathBuf::from("/nix/store/jjjj-umu-launcher-1.4.4/bin/umu-run"),
            PathBuf::from("/nix/store/hhhh-umu-launcher-1.4.4-bwrap"),
        ]
    );
}

#[test]
fn umu_missing_and_rejected() {
    let fs = MemFs::new();
    let roots = fhs_roots();
    assert_eq!(
        locate(&fs, &locate_input(&roots, None, &[])),
        Err(LocateError::NotFound)
    );

    // A pipx venv inside Zaun's data directory would be writable by games.
    host_trees::umu_pipx(&fs, "/home/sam");
    let path = std::ffi::OsString::from("/home/sam/.local/bin");
    let dirs = [PathBuf::from("/home/sam/.local/share")];
    assert!(matches!(
        locate(&fs, &locate_input(&roots, Some(&path), &dirs)),
        Err(LocateError::ClosureRejected { .. })
    ));

    let mut problems = Vec::new();
    let runner = ScriptedRunner::new();
    assert!(
        probe_umu(
            &MemFs::new(),
            &runner,
            &locate_input(&roots, None, &[]),
            "/usr/bin",
            &mut problems
        )
        .is_none()
    );
    assert_eq!(problems[0].code, ErrorCode::HostUmuNotFound);
}

#[test]
fn umu_version_check() {
    let fs = MemFs::new();
    host_trees::umu_system(&fs);
    let roots = fhs_roots();
    for (out, code) in [
        ("umu-launcher version 1.4.4 (3.14.10 (main))\n", None),
        (
            "umu-launcher version 1.3.0 (3.12)\n",
            Some(ErrorCode::HostUmuTooOld),
        ),
        ("Traceback\n", Some(ErrorCode::HostUmuTooOld)),
    ] {
        let runner =
            ScriptedRunner::new().expect(&["/usr/bin/umu-run", "--version"], CannedOutput::ok(out));
        let mut problems = Vec::new();
        let u = probe_umu(
            &fs,
            &runner,
            &locate_input(&roots, None, &[]),
            "/usr/bin",
            &mut problems,
        )
        .unwrap();
        assert_eq!(problems.first().map(|p| p.code), code, "{out}");
        assert_eq!(u.version_accepted(), code.is_none());
    }
}

// ---- bwrap ----------------------------------------------------------------------------------

const BWRAP: &str = "/usr/bin/bwrap";

fn argv(args: Vec<std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    std::iter::once(BWRAP.into()).chain(args).collect()
}

fn bwrap_runner(help: &str, userns: CannedOutput, nested: CannedOutput) -> ScriptedRunner {
    ScriptedRunner::new()
        .expect(
            &[BWRAP, "--version"],
            CannedOutput::ok("bubblewrap 0.12.0\n"),
        )
        .expect(&[BWRAP, "--help"], CannedOutput::ok(help))
        .expect(&argv(userns_args()), userns)
        .expect(&argv(nested_args(Path::new(BWRAP))), nested)
        .expect(&argv(overlay_args(Path::new("/tmp"))), CannedOutput::ok(""))
}

fn run_bwrap(
    fs: &MemFs,
    runner: &ScriptedRunner,
    os: &OsRelease,
    kernel: &KernelFacts,
) -> (
    Option<zaun_core::host::BwrapFacts>,
    Vec<zaun_core::host::HostProblem>,
) {
    let mut problems = Vec::new();
    let b = probe_bwrap(
        fs,
        runner,
        &BwrapProbeInput {
            host_path: None,
            probe_path: "/usr/bin:/bin",
            os,
            kernel,
            overlay_dir: Path::new("/tmp"),
        },
        &mut problems,
    );
    (b, problems)
}

fn fs_with_bwrap() -> MemFs {
    let fs = MemFs::new();
    host_trees::arch(&fs);
    fs.dir("/tmp");
    fs
}

#[test]
fn bwrap_all_good() {
    let fs = fs_with_bwrap();
    let r = bwrap_runner(HELP_0_12, CannedOutput::ok(""), CannedOutput::ok(""));
    let (b, problems) = run_bwrap(&fs, &r, &OsRelease::default(), &KernelFacts::default());
    let b = b.unwrap();
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(b.version, Some(ToolVersion::new(0, 12, 0)));
    assert!(b.userns_ok && b.nested_userns_ok && b.overlay_ok && !b.setuid);
    assert!(b.has(BwrapFlag::Args));
    // The probe commands use the layout PATH.
    let calls = r.calls();
    assert_eq!(calls.len(), 5);
}

#[test]
fn bwrap_failure_codes() {
    let fs = fs_with_bwrap();
    let ubuntu = OsRelease {
        id: Some("ubuntu".into()),
        id_like: vec!["debian".into()],
        ..OsRelease::default()
    };
    // ZN-HOST-004 with the AppArmor hint.
    let r = bwrap_runner(
        HELP_0_12,
        CannedOutput::fail(1, "bwrap: setting up uid map: Permission denied\n"),
        CannedOutput::ok(""),
    );
    let k = KernelFacts {
        apparmor_restrict_unprivileged_userns: Some(1),
        ..KernelFacts::default()
    };
    let (b, p) = run_bwrap(&fs, &r, &ubuntu, &k);
    assert!(!b.unwrap().userns_ok);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].code, ErrorCode::HostUsernsUnavailable);
    assert!(matches!(p[0].hint, Some(HostHint::AppArmor { .. })));
    assert!(p[0].detail.contains("uid map"));

    // ZN-HOST-005.
    let r = bwrap_runner(
        HELP_0_12,
        CannedOutput::ok(""),
        CannedOutput::fail(
            1,
            "bwrap: Creating new namespace failed: nesting depth or /proc/sys/user/max_*_namespaces exceeded\n",
        ),
    );
    let (b, p) = run_bwrap(&fs, &r, &OsRelease::default(), &KernelFacts::default());
    let b = b.unwrap();
    assert!(b.userns_ok && !b.nested_userns_ok);
    assert_eq!(p[0].code, ErrorCode::HostNestedUsernsUnavailable);

    // ZN-HOST-005 on timeout too.
    let r = bwrap_runner(HELP_0_12, CannedOutput::ok(""), CannedOutput::timeout());
    let (_, p) = run_bwrap(&fs, &r, &OsRelease::default(), &KernelFacts::default());
    assert!(p[0].detail.contains("timed out"));

    // ZN-HOST-003: setuid bwrap without user namespaces.
    fs.chown("/usr/bin/bwrap", 0, 0);
    fs.set_mode(Path::new("/usr/bin/bwrap"), 0o4755).unwrap();
    let r = bwrap_runner(
        HELP_0_12,
        CannedOutput::fail(1, "bwrap: No permissions to create new namespace"),
        CannedOutput::ok(""),
    );
    let (b, p) = run_bwrap(&fs, &r, &OsRelease::default(), &KernelFacts::default());
    assert!(b.unwrap().setuid);
    assert_eq!(p[0].code, ErrorCode::HostBwrapSetuidOnly);
}

#[test]
fn bwrap_old_version_has_no_overlay() {
    let fs = fs_with_bwrap();
    let r = bwrap_runner(HELP_0_9, CannedOutput::ok(""), CannedOutput::ok(""));
    let (b, p) = run_bwrap(&fs, &r, &OsRelease::default(), &KernelFacts::default());
    assert!(p.is_empty());
    assert!(!b.unwrap().overlay_ok);
    assert!(
        r.unused()
            .iter()
            .any(|a| a.iter().any(|x| x == "--tmp-overlay")),
        "overlay probe skipped"
    );
}

#[test]
fn bwrap_missing_option_and_missing_binary() {
    let fs = fs_with_bwrap();
    let help: String = HELP_0_12
        .lines()
        .filter(|l| !l.contains("--sync-fd"))
        .collect::<Vec<_>>()
        .join("\n");
    let r = bwrap_runner(&help, CannedOutput::ok(""), CannedOutput::ok(""));
    let (_, p) = run_bwrap(&fs, &r, &OsRelease::default(), &KernelFacts::default());
    assert_eq!(p[0].code, ErrorCode::HostBwrapMissingOption);
    assert!(p[0].detail.contains("--sync-fd"));

    let (b, p) = run_bwrap(
        &MemFs::new(),
        &ScriptedRunner::new(),
        &OsRelease::default(),
        &KernelFacts::default(),
    );
    assert!(b.is_none());
    assert_eq!(p[0].code, ErrorCode::HostBwrapNotFound);
}

// ---- orchestration and cache ----------------------------------------------------------------

fn deps(fs: Arc<MemFs>, runner: Arc<ScriptedRunner>) -> ProbeDeps {
    let env = PathEnv::new(1000)
        .with("HOME", "/home/sam")
        .with("XDG_RUNTIME_DIR", "/run/user/1000")
        .with("PATH", "/usr/bin");
    ProbeDeps {
        access: Arc::new(ModeAccess::new(fs.clone(), 1000, vec![1000])),
        fs,
        runner,
        clock: Arc::new(FakeClock::baseline()),
        paths: AppPaths::resolve(&env).unwrap(),
        env,
        umu_override: None,
        user_name: Some("sam".into()),
        jobs: None,
        compositor: Arc::new(NoCompositor),
    }
}

fn full_host() -> (Arc<MemFs>, Arc<ScriptedRunner>) {
    let fs = Arc::new(MemFs::new());
    host_trees::arch(&fs);
    host_trees::umu_system(&fs);
    host_trees::input_devices(&fs);
    fs.dir("/tmp");
    fs.file(
        "/proc/sys/kernel/random/boot_id",
        "6ff0c7df-1ecf-41ff-ace1-7052145d2675\n",
    );
    let r = bwrap_runner(HELP_0_12, CannedOutput::ok(""), CannedOutput::ok("")).expect(
        &["/usr/bin/umu-run", "--version"],
        CannedOutput::ok("umu-launcher version 1.4.4 (3.14)\n"),
    );
    (fs, Arc::new(r))
}

#[test]
fn probe_all_and_serde_round_trip() {
    let (fs, r) = full_host();
    let facts = probe_all(&deps(fs, r));
    assert!(facts.can_sandbox());
    assert_eq!(facts.os.id.as_deref(), Some("arch"));
    assert!(facts.umu.as_ref().unwrap().version_accepted());
    // No display or GPU in this fixture: reported as problems, not failures of the probe.
    assert!(facts.problem(ErrorCode::HostNoDisplay).is_some());
    assert!(facts.problem(ErrorCode::HostNoGpu).is_some());
    assert!(!facts.notes.is_empty());
    let json = serde_json::to_string_pretty(&facts).unwrap();
    let back: HostFacts = serde_json::from_str(&json).unwrap();
    assert_eq!(back, facts);
    assert!(json.contains("\"--unshare-user\""));
}

#[test]
fn cache_is_reused_and_invalidated_by_replacing_bwrap() {
    let (fs, r) = full_host();
    let d = deps(fs.clone(), r.clone());
    let probe = LiveHostProbe::new(d.clone());
    let first = probe.facts();
    let calls = r.calls().len();
    assert!(fs.exists(&d.paths.host_facts_file));

    // A second start uses the cache: no commands run.
    let second = LiveHostProbe::new(d.clone()).facts();
    assert_eq!(*second, *first);
    assert_eq!(r.calls().len(), calls);

    // Replacing bwrap changes its stamp: the cache no longer applies.
    fs.set_now(1_900_000_000_000_000_000);
    fs.file("/usr/bin/bwrap", "\x7fELF new build");
    fs.set_mode(Path::new("/usr/bin/bwrap"), 0o755).unwrap();
    assert!(load_cache(&*fs, &d.paths.host_facts_file, None).is_none());
    let p3 = LiveHostProbe::new(d.clone());
    p3.facts();
    assert!(r.calls().len() > calls);
    assert!(p3.revalidate().is_valid());

    // A different boot invalidates it as well.
    save_cache(&*fs, &d.paths.host_facts_file, &first, None).unwrap();
    fs.file("/proc/sys/kernel/random/boot_id", "another-boot\n");
    assert!(load_cache(&*fs, &d.paths.host_facts_file, None).is_none());
}

#[test]
fn revalidation_detects_changed_binaries() {
    let (fs, r) = full_host();
    let d = deps(fs.clone(), r);
    let probe = LiveHostProbe::new(d);
    let facts = probe.refresh();
    assert!(probe.revalidate().is_valid());
    fs.set_now(1_900_000_000_000_000_000);
    fs.file("/usr/bin/umu-run", "#!/usr/bin/python3\n# updated\n");
    let rv = probe.revalidate();
    assert!(!rv.is_valid());
    assert!(rv.changed[0].contains("umu-run"));
    let key = CacheKey {
        format: 0,
        app_version: String::new(),
        boot_id: String::new(),
        umu_override: None,
        bwrap: None,
        umu: None,
    };
    assert!(!revalidate(&*fs, &facts, &key).is_valid());
}

// ---- the compositor's security contexts (ADR-0029) ----------------------------------------

/// Answers for the compositor and records which socket it was asked about.
struct Compositor {
    offers: bool,
    asked: parking_lot::Mutex<Vec<PathBuf>>,
}

impl zaun_core::host::CompositorProbe for Compositor {
    fn offers_security_context(&self, display: &Path) -> bool {
        self.asked.lock().push(display.to_owned());
        self.offers
    }
}

fn wayland_deps(offers: bool) -> (Arc<MemFs>, ProbeDeps, Arc<Compositor>) {
    let (fs, r) = full_host();
    fs.socket("/run/user/1000/wayland-1");
    let compositor = Arc::new(Compositor {
        offers,
        asked: parking_lot::Mutex::new(Vec::new()),
    });
    let mut d = deps(fs.clone(), r);
    d.env = d.env.with("WAYLAND_DISPLAY", "wayland-1");
    d.compositor = compositor.clone();
    (fs, d, compositor)
}

#[test]
fn the_probe_asks_the_compositor_about_security_contexts() {
    for offers in [true, false] {
        let (_, d, compositor) = wayland_deps(offers);
        let facts = probe_all(&d);
        let w = facts.display.wayland.as_ref().expect("Wayland socket");
        assert_eq!(w.security_context, offers);
        assert_ne!(w.inode, 0);
        assert_eq!(
            *compositor.asked.lock(),
            vec![PathBuf::from("/run/user/1000/wayland-1")]
        );
        assert_eq!(
            facts
                .notes
                .iter()
                .any(|n| n.contains("does not offer security contexts")),
            !offers
        );
    }
}

#[test]
fn without_a_wayland_socket_the_compositor_is_not_asked() {
    let (fs, r) = full_host();
    let compositor = Arc::new(Compositor {
        offers: true,
        asked: parking_lot::Mutex::new(Vec::new()),
    });
    let mut d = deps(fs, r);
    d.compositor = compositor.clone();
    assert!(probe_all(&d).display.wayland.is_none());
    assert!(compositor.asked.lock().is_empty());
}

#[test]
fn revalidation_notices_a_restarted_compositor() {
    let (fs, d, _) = wayland_deps(true);
    let probe = LiveHostProbe::new(d);
    probe.refresh();
    assert!(probe.revalidate().is_valid());
    // A restarted (or different) compositor makes a new socket at the same path.
    fs.socket("/run/user/1000/wayland-1");
    let rv = probe.revalidate();
    assert!(
        rv.changed.iter().any(|c| c.contains("was replaced")),
        "{rv:?}"
    );
}
