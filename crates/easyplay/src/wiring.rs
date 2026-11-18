//! Real implementations of every seam, assembled into `service::ServiceDeps` for the GUI and
//! the headless verbs (MODULES §9 `run_gui.rs`: "construct real implementations of all
//! traits"). Nothing here probes the host: the AppService does that on the job pool.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use easyplay_core::artwork::http::{HttpClient, UreqTransport};
use easyplay_core::artwork::steam::FsAppIdFile;
use easyplay_core::artwork::{ArtIndex, ImageCodec, ImageCrateCodec, PeIconSource};
use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::config::{self, EnvOverrides, LoadOptions, Preferences, RuntimeDefault};
use easyplay_core::error::{Error, ErrorCode};
use easyplay_core::fsx::{Fs, MountTable, RealFs};
use easyplay_core::host::{
    HostFacts, HostProbe, LiveCompositor, LiveHostProbe, ProbeDeps, RealAccess, RealRunner,
    Revalidation,
};
use easyplay_core::identity::{FsPeInspector, PeInspector, SharedLocations};
use easyplay_core::jobs::{JobPool, JobPoolConfig};
use easyplay_core::model::{Channel, RuntimeRef};
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::procfs::{ProcSource, RealProc};
use easyplay_core::runtime::{LocalRuntimeService, MemCatalogStore, RuntimeSettings, RuntimeStore};
use easyplay_core::service::flows::runtime::CatalogUmuDb;
use easyplay_core::service::{LaunchEnv, ServiceDeps};
use easyplay_core::session::{FlockLocks, RealLauncher};

use crate::game_source::{group_name, init_binary};

/// The live probe with the `EASYPLAY_BWRAP` override applied to its facts.
struct OverrideProbe {
    inner: LiveHostProbe,
    bwrap: Option<PathBuf>,
}

impl OverrideProbe {
    fn apply(&self, f: Arc<HostFacts>) -> Arc<HostFacts> {
        match (&self.bwrap, f.bwrap.is_some()) {
            (Some(p), true) => {
                let mut f = (*f).clone();
                if let Some(b) = f.bwrap.as_mut() {
                    b.path = p.clone();
                }
                Arc::new(f)
            }
            _ => f,
        }
    }
}

impl HostProbe for OverrideProbe {
    fn facts(&self) -> Arc<HostFacts> {
        self.apply(self.inner.facts())
    }
    fn refresh(&self) -> Arc<HostFacts> {
        self.apply(self.inner.refresh())
    }
    fn revalidate(&self) -> Revalidation {
        self.inner.revalidate()
    }
    fn runner(&self) -> Option<Arc<dyn easyplay_core::host::CommandRunner>> {
        self.inner.runner()
    }
}

fn runtime_ref(d: &RuntimeDefault) -> RuntimeRef {
    match d {
        RuntimeDefault::UmuProton => RuntimeRef::Channel(Channel::UmuProton),
        RuntimeDefault::GeProton => RuntimeRef::Channel(Channel::GeProton),
        RuntimeDefault::Pinned(n) => RuntimeRef::Pinned(n.clone()),
        RuntimeDefault::External(p) => RuntimeRef::External(p.clone()),
    }
}

/// Everything the GUI and the headless verbs share.
pub struct Wiring {
    /// The service's dependencies (taken once by [`Wiring::take_deps`]).
    deps: Option<ServiceDeps>,
    /// Job pool (also used by the thumbnail service).
    pub jobs: Arc<JobPool>,
    /// Filesystem.
    pub fs: Arc<dyn Fs>,
    /// Paths.
    pub paths: AppPaths,
    /// Image codec.
    pub codec: Arc<dyn ImageCodec>,
    /// Hash → original (shared with the thumbnail service).
    pub art_index: Arc<ArtIndex>,
}

impl Wiring {
    /// The service's dependencies (once); the shared parts stay here.
    pub fn take_deps(&mut self) -> Result<ServiceDeps, Error> {
        self.deps.take().ok_or_else(|| {
            Error::new(ErrorCode::IntTaskFailed).with("detail", "service dependencies taken twice")
        })
    }
}

/// Load the preferences (missing file → defaults; problems are logged).
pub(crate) fn load_prefs(fs: &dyn Fs, paths: &AppPaths, clock: &dyn Clock) -> Preferences {
    match config::load::load(
        fs,
        &paths.config_file,
        clock.now(),
        &LoadOptions { env_rule: None },
    ) {
        Ok(l) => {
            for w in &l.warnings {
                tracing::warn!(key = %w.key, problem = ?w.problem, "preference ignored");
            }
            l.prefs
        }
        Err(e) => {
            tracing::warn!(error = %e, "preferences not readable; using defaults");
            Preferences::default()
        }
    }
}

/// Assemble the real dependencies.
pub fn build() -> Result<Wiring, Error> {
    build_with(None)
}

/// [`build`] with another `easyplay` binary as the supervisor (tests run from a test
/// binary, which cannot `supervise`).
pub fn build_with(supervisor: Option<PathBuf>) -> Result<Wiring, Error> {
    let env = PathEnv::from_process();
    let (over, warnings) = EnvOverrides::from_env(&env);
    for w in warnings {
        tracing::warn!(var = w.var, reason = %w.reason, "environment override ignored");
    }
    let paths = AppPaths::resolve(&env)
        .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("paths", e))?;
    let home = paths
        .home
        .clone()
        .ok_or_else(|| Error::new(ErrorCode::IntTaskFailed).with("detail", "HOME is not set"))?;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let fs: Arc<dyn Fs> = Arc::new(RealFs::new());
    let jobs = Arc::new(
        JobPool::new(JobPoolConfig::default(), Arc::clone(&clock))
            .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))?,
    );
    let prefs = load_prefs(&*fs, &paths, &*clock);
    let user_name = env
        .get("USER")
        .or_else(|| env.get("LOGNAME"))
        .and_then(|u| u.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| "user".into());
    let host: Arc<dyn HostProbe> = Arc::new(OverrideProbe {
        inner: LiveHostProbe::new(ProbeDeps {
            fs: Arc::clone(&fs),
            runner: Arc::new(RealRunner::new(Arc::clone(&clock))),
            access: Arc::new(RealAccess),
            clock: Arc::clone(&clock),
            env: env.clone(),
            paths: paths.clone(),
            umu_override: over.umu_run.clone(),
            user_name: Some(user_name.clone()),
            jobs: Some(Arc::clone(&jobs)),
            compositor: Arc::new(LiveCompositor),
        }),
        bwrap: over.bwrap.clone(),
    });
    let proc: Arc<dyn ProcSource> = Arc::new(RealProc::new(Arc::clone(&fs)));
    let exe = match supervisor {
        Some(p) => p,
        None => std::env::current_exe()
            .map_err(|e| Error::new(ErrorCode::SesSupervisorNotStarted).with_source(e))?,
    };
    let launcher = Arc::new(RealLauncher::new(
        exe,
        paths.clone(),
        Arc::clone(&fs),
        Arc::clone(&proc),
        Arc::clone(&clock),
    ));
    // Isolation mode needs to know whether the store can reflink (RUNTIME_MANAGEMENT §7).
    let store = RuntimeStore::new(&paths);
    let reflink_ok = fs
        .create_dir_all(&store.session_var, 0o700)
        .and_then(|()| fs.reflink_supported(&store.root, &store.session_var))
        .ok();
    let runtimes: Arc<dyn easyplay_core::runtime::RuntimeService> =
        Arc::new(LocalRuntimeService::new(
            Arc::clone(&fs),
            Arc::clone(&clock),
            Arc::new(MemCatalogStore::new()),
            store,
            home.clone(),
            RuntimeSettings {
                default: runtime_ref(&prefs.runtime.default),
                extra_search_dirs: prefs.runtime.extra_search_dirs.clone(),
                always_isolate: prefs.protection.always_isolate_runtime,
                reflink_ok,
            },
            Arc::clone(&jobs),
        ));
    let pe: Arc<dyn PeInspector> =
        Arc::new(FsPeInspector::new(Arc::clone(&fs), Arc::clone(&clock)));
    let codec: Arc<dyn ImageCodec> = Arc::new(ImageCrateCodec);
    let mounts = MountTable::read(&*fs)?;
    let gid = rustix::process::getgid().as_raw();
    let group_name = group_name(&*fs, gid, &user_name);
    let host_env: BTreeMap<String, String> = std::env::vars().collect();
    let launch_env = LaunchEnv {
        uid: env.uid(),
        gid,
        user_name,
        group_name,
        home,
        host_env,
        init_binary: init_binary(over.init.as_deref(), &*fs),
        mounts: mounts.clone(),
        user_dirs: Vec::new(),
        seccomp_available: cfg!(feature = "seccomp"),
        global_env: prefs.launch.global_env.clone(),
    };
    let shared = SharedLocations::gather(&*fs, &env, &paths, &mounts);
    let art_index = Arc::new(ArtIndex::new());
    let http = Arc::new(HttpClient::new(
        Arc::new(UreqTransport::new()),
        Arc::clone(&clock),
    ));
    let desktop = easyplay_core::desktop::DesktopServices::real(
        Arc::clone(&fs),
        Arc::new(RealRunner::new(Arc::clone(&clock))),
        &env,
        &paths,
    )
    .map(Arc::new);
    let deps = ServiceDeps {
        fs: Arc::clone(&fs),
        clock: Arc::clone(&clock),
        paths: paths.clone(),
        jobs: Arc::clone(&jobs),
        proc,
        launcher,
        locks: Arc::new(FlockLocks),
        umu_titles: Arc::new(CatalogUmuDb::new(Arc::clone(&fs), Arc::clone(&runtimes))),
        runtimes,
        host,
        pe: Arc::clone(&pe),
        icons: Arc::new(PeIconSource::new(pe)),
        codec: Some(Arc::clone(&codec)),
        launch_env,
        prefs,
        desktop,
        http: Some(http),
        app_id_file: Arc::new(FsAppIdFile::new(Arc::clone(&fs))),
        art_index: Arc::clone(&art_index),
        shared,
        policy_check: None,
        watch_sessions: true,
        chat: chat_factory(&env, &*fs, &paths, over.bwrap.clone()),
        overlay: overlay_factory(&env, &*fs, &paths, over.bwrap.clone()),
        media: Some(media_factory()),
    };
    Ok(Wiring {
        deps: Some(deps),
        jobs,
        fs,
        paths,
        codec,
        art_index,
    })
}

/// Calls' media (ADR-0035): WebRTC through str0m on its own threads, sound through the
/// system's PulseAudio (or PipeWire's) server. It also carries game streams (ADR-0036);
/// shared windows come from the ScreenCast portal in a build with the `portal` feature.
fn media_factory() -> Arc<dyn easyplay_core::chat::media::MediaFactory> {
    Arc::new(easyplay_media::RtcMedia::new(Arc::new(
        easyplay_media::PulseAudio,
    )))
}

/// The Chat engine (ADR-0033), when this build has one: `easyplay-chat` beside
/// this program or in the libexec folder, and `libsimplex.so` from `EASYPLAY_SIMPLEX_LIB`
/// (packages set it) or `<libexec>/simplex/libsimplex.so`. `EASYPLAY_SIMPLEX_LIBRARY_PATH`
/// is passed to the helper as `LD_LIBRARY_PATH`. `None` when either is missing: the
/// Chat page then says this build does not include it.
fn chat_factory(
    env: &PathEnv,
    fs: &dyn Fs,
    paths: &AppPaths,
    bwrap: Option<PathBuf>,
) -> Option<Arc<dyn easyplay_core::chat::EngineFactory>> {
    use easyplay_core::consts::LIBEXEC_DIR;
    const HELPER: &str = "easyplay-chat";
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let executable = |p: &PathBuf| {
        fs.metadata_follow(p)
            .is_ok_and(|m| m.is_file() && m.mode & 0o111 != 0)
    };
    let helper = exe_dir
        .map(|d| d.join(HELPER))
        .into_iter()
        .chain([PathBuf::from(LIBEXEC_DIR).join(HELPER)])
        .find(|p| executable(p))?;
    let library = env
        .get("EASYPLAY_SIMPLEX_LIB")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(LIBEXEC_DIR)
                .join("simplex")
                .join("libsimplex.so")
        });
    let bwrap =
        bwrap.or_else(|| easyplay_core::host::bwrap_probe::locate_bwrap(fs, env.get("PATH")));
    Some(Arc::new(easyplay_core::chat::SimplexFactory {
        helper,
        library,
        bwrap,
        library_path: env.get("EASYPLAY_SIMPLEX_LIBRARY_PATH").map(Into::into),
        log: paths.logs_dir.join(easyplay_core::chat::simplex::LOG_NAME),
    }))
}

/// The in-game overlay (ADR-0037): `easyplay-overlay` beside this program or in the
/// libexec folder, drawn on the compositor named by `WAYLAND_DISPLAY`. Without a Wayland
/// display (X11, or the resident service started outside the session) there is no overlay.
fn overlay_factory(
    env: &PathEnv,
    fs: &dyn Fs,
    paths: &AppPaths,
    bwrap: Option<PathBuf>,
) -> Option<Arc<dyn easyplay_core::chat::overlay::OverlayFactory>> {
    use easyplay_core::consts::LIBEXEC_DIR;
    const HELPER: &str = "easyplay-overlay";
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from));
    let helper = exe_dir
        .map(|d| d.join(HELPER))
        .into_iter()
        .chain([PathBuf::from(LIBEXEC_DIR).join(HELPER)])
        .find(|p| {
            fs.metadata_follow(p)
                .is_ok_and(|m| m.is_file() && m.mode & 0o111 != 0)
        })?;
    let wayland = env
        .get("WAYLAND_DISPLAY")
        .filter(|d| !d.is_empty())
        .and_then(|d| {
            let d = PathBuf::from(d);
            if d.is_absolute() {
                Some(d)
            } else {
                env.get("XDG_RUNTIME_DIR").map(|r| PathBuf::from(r).join(d))
            }
        });
    // NVIDIA's nodes; `/dev/dri` is bound as a whole.
    let gpu_nodes = fs
        .read_dir(std::path::Path::new("/dev"))
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.name.to_string_lossy().starts_with("nvidia"))
        .map(|e| e.path)
        .filter(|p| !p.is_dir())
        .collect();
    let bwrap =
        bwrap.or_else(|| easyplay_core::host::bwrap_probe::locate_bwrap(fs, env.get("PATH")));
    Some(Arc::new(
        easyplay_core::chat::overlay::HelperOverlayFactory {
            helper,
            bwrap,
            wayland,
            gpu_nodes,
            library_path: env.get("LD_LIBRARY_PATH").map(Into::into),
            log: paths
                .logs_dir
                .join(easyplay_core::chat::overlay::helper::LOG_NAME),
        },
    ))
}
