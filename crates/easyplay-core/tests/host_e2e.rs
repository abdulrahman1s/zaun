//! Read-only probes of the real host (`EASYPLAY_E2E=1`). Nothing is written: the probe is run
//! without its cache and discovery only lists directories.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use easyplay_core::clock::SystemClock;
use easyplay_core::fsx::{Fs as _, RealFs};
use easyplay_core::host::{LayoutKind, ProbeDeps, RealAccess, RealRunner, probe_all};
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::runtime::discovery::DiscoveryInput;
use easyplay_core::runtime::{RuntimeStore, discover};

fn enabled() -> bool {
    std::env::var_os("EASYPLAY_E2E").is_some_and(|v| v == "1")
}

#[test]
fn real_host_probe() {
    if !enabled() {
        return;
    }
    let env = PathEnv::from_process();
    let clock = Arc::new(SystemClock::new());
    let deps = ProbeDeps {
        fs: Arc::new(RealFs::new()),
        runner: Arc::new(RealRunner::new(clock.clone())),
        access: Arc::new(RealAccess),
        clock,
        paths: AppPaths::resolve(&env).unwrap(),
        user_name: env.get("USER").and_then(|u| u.to_str()).map(str::to_owned),
        env,
        umu_override: None,
        jobs: None,
    };
    let facts = probe_all(&deps);
    let b = facts.bwrap.as_ref().expect("bwrap present");
    assert!(
        b.missing_required().is_empty(),
        "{:?}",
        b.missing_required()
    );
    assert!(b.userns_ok && b.nested_userns_ok, "{:?}", facts.problems);
    if facts.os.id.as_deref() == Some("nixos") {
        assert_eq!(facts.layout.kind, LayoutKind::Nix);
        assert!(facts.etc_links.links.values().any(|l| l.via_etc_static));
    }
    assert!(facts.umu.is_some(), "{:?}", facts.problems);
    eprintln!("{}", serde_json::to_string_pretty(&facts).unwrap());
}

#[test]
fn real_runtime_discovery() {
    if !enabled() {
        return;
    }
    let env = PathEnv::from_process();
    let paths = AppPaths::resolve(&env).unwrap();
    let store = RuntimeStore::new(&paths);
    let d = discover(
        &RealFs::new(),
        &DiscoveryInput {
            store: &store,
            home: paths.home.as_deref(),
            extra_dirs: &[],
            now: easyplay_core::clock::Timestamp::EPOCH,
        },
    );
    for b in &d.builds {
        eprintln!(
            "{} {:?} {:?} {}",
            b.name,
            b.origin,
            b.required_slr,
            b.path.display()
        );
    }
    for r in &d.rejected {
        eprintln!("rejected {}: {}", r.path.display(), r.reason);
    }
}

/// The session's sound server (ADR-0026) starts from the probed `pipewire` program, listens
/// on its socket, and stops when told to. Nothing plays and nothing records.
#[test]
fn real_sound_server_starts_and_stops() {
    use easyplay_core::host::audio::probe_audio;
    use easyplay_core::sandbox::sound_socket;
    use easyplay_core::session::{RealSpawner, SandboxSpawner, SoundServer, write_sound_config};

    if !enabled() {
        return;
    }
    let env = PathEnv::from_process();
    let audio = probe_audio(&RealFs::new(), &env, &mut Vec::new());
    let (Some(program), Some(remote)) = (&audio.pipewire_program, &audio.pipewire_socket) else {
        eprintln!("no PipeWire here: {audio:?}");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("sound");
    let log = tmp.path().join("sound-server.log");
    let uid = rustix::process::getuid().as_raw();
    let config = write_sound_config(&RealFs::new(), uid, &dir).unwrap();
    let socket = sound_socket(&dir);
    let mut server = RealSpawner
        .start_sound_server(SoundServer {
            program,
            config: &config,
            socket: &socket,
            remote,
            log: &log,
        })
        .unwrap_or_else(|e| {
            panic!(
                "{e}\n{}",
                String::from_utf8_lossy(&RealFs::new().read(&log, 1 << 20).unwrap_or_default())
            )
        });
    assert!(socket.exists());
    assert_eq!(server.try_wait().unwrap(), None, "still running");
    server.kill_monitor().unwrap();
}
