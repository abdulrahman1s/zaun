//! Probe orchestration and the `host-facts.json` cache (MODULES §4, PERFORMANCE §7).
//!
//! The layout probe runs first (its `PATH` and system roots feed the others); then the bwrap,
//! umu and GPU probes (the ones that run commands) run in parallel on the job pool while the
//! cheap filesystem probes run on the calling thread. The result is cached keyed by format,
//! EasyPlay version, boot ID, the umu preference and `stat` of the `bwrap` and `umu-run`
//! binaries.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::access::AccessCheck;
use super::audio::probe_audio;
use super::bwrap_probe::{BwrapProbeInput, probe_bwrap, read_kernel_facts};
use super::display::probe_display;
use super::etc_links::probe_etc_links;
use super::facts::{HOST_FACTS_FORMAT, HostFacts, HostProblem, StoreFsFacts};
use super::gpu::probe_gpu;
use super::input::probe_input;
use super::layout::{LayoutInput, probe_layout, read_os_release};
use super::runner::CommandRunner;
use super::sysfs::probe_sysfs;
use super::umu_probe::{LocateInput, probe_umu};
use crate::clock::Clock;
use crate::consts::APP_VERSION;
use crate::error::{Error, ErrorCode};
use crate::fsx::{FileType, Fs, MountTable};
use crate::jobs::{JobLabel, JobPool, JobPriority};
use crate::paths::{AppPaths, PathEnv};

/// Everything a probe needs. Cheap to clone.
#[derive(Clone)]
pub struct ProbeDeps {
    /// Filesystem.
    pub fs: Arc<dyn Fs>,
    /// Command runner.
    pub runner: Arc<dyn CommandRunner>,
    /// Device access checks.
    pub access: Arc<dyn AccessCheck>,
    /// Clock.
    pub clock: Arc<dyn Clock>,
    /// Host environment.
    pub env: PathEnv,
    /// EasyPlay's paths.
    pub paths: AppPaths,
    /// Preference `paths.umu_run`.
    pub umu_override: Option<PathBuf>,
    /// User name (for NixOS per-user profiles).
    pub user_name: Option<String>,
    /// Job pool for the parallel sub-probes; `None` runs them sequentially.
    pub jobs: Option<Arc<JobPool>>,
}

fn env_os(env: &PathEnv, key: &str) -> Option<OsString> {
    env.get(key).map(ToOwned::to_owned)
}

/// `/etc/machine-id` when it holds 32 hex digits.
fn read_machine_id(fs: &dyn Fs) -> Option<String> {
    let bytes = fs.read_follow(Path::new("/etc/machine-id"), 64).ok()?;
    let id = String::from_utf8(bytes).ok()?.trim().to_ascii_lowercase();
    (id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

/// `/dev/video*` character devices, sorted.
fn list_video_nodes(fs: &dyn Fs) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs
        .read_dir(Path::new("/dev"))
        .unwrap_or_default()
        .into_iter()
        .filter(|e| {
            e.name.to_string_lossy().starts_with("video") && e.file_type == FileType::CharDevice
        })
        .map(|e| e.path)
        .collect();
    v.sort();
    v
}

/// Run every sub-probe.
pub fn probe_all(deps: &ProbeDeps) -> HostFacts {
    let fs = &*deps.fs;
    let mut facts = HostFacts::empty(deps.clock.now());
    facts.os = read_os_release(fs);
    facts.kernel = read_kernel_facts(fs);
    facts.machine_id = read_machine_id(fs);
    facts.video_nodes = list_video_nodes(fs);
    facts.layout = probe_layout(
        fs,
        &LayoutInput {
            os: Some(&facts.os),
            user_name: deps.user_name.as_deref(),
        },
    );

    let bwrap_job = {
        let d = deps.clone();
        let os = facts.os.clone();
        let kernel = facts.kernel.clone();
        let probe_path = facts.layout.path_env.clone();
        move || {
            let mut problems = Vec::new();
            let host_path = env_os(&d.env, "PATH");
            let overlay_dir = PathBuf::from("/tmp");
            let b = probe_bwrap(
                &*d.fs,
                &*d.runner,
                &BwrapProbeInput {
                    host_path: host_path.as_deref(),
                    probe_path: &probe_path,
                    os: &os,
                    kernel: &kernel,
                    overlay_dir: &overlay_dir,
                },
                &mut problems,
            );
            (b, problems)
        }
    };
    let umu_job = {
        let d = deps.clone();
        let roots = facts.layout.system_roots();
        let probe_path = facts.layout.path_env.clone();
        move || {
            let mut problems = Vec::new();
            let host_path = env_os(&d.env, "PATH");
            let easyplay_dirs = [
                d.paths.config_dir.clone(),
                d.paths.data_dir.clone(),
                d.paths.cache_dir.clone(),
                d.paths.state_dir.clone(),
            ];
            let u = probe_umu(
                &*d.fs,
                &*d.runner,
                &LocateInput {
                    preference: d.umu_override.as_deref(),
                    host_path: host_path.as_deref(),
                    system_roots: &roots,
                    home: d.paths.home.as_deref(),
                    easyplay_dirs: &easyplay_dirs,
                },
                &probe_path,
                &mut problems,
            );
            (u, problems)
        }
    };
    let gpu_job = {
        let d = deps.clone();
        let kind = facts.layout.kind;
        move || probe_gpu(&*d.fs, &*d.access, &*d.runner, kind)
    };

    let ((bwrap, mut bwrap_problems), (umu, mut umu_problems), gpu) = match &deps.jobs {
        Some(pool) => {
            let hb = pool.submit(
                JobPriority::Interactive,
                JobLabel("host-probe-bwrap"),
                move |_| bwrap_job(),
            );
            let hu = pool.submit(
                JobPriority::Interactive,
                JobLabel("host-probe-umu"),
                move |_| umu_job(),
            );
            let hg = pool.submit(
                JobPriority::Interactive,
                JobLabel("host-probe-gpu"),
                move |_| gpu_job(),
            );
            // Fall back to "nothing found" when a sub-probe panicked.
            let b = hb.wait().unwrap_or_else(|_| {
                (
                    None,
                    vec![HostProblem::new(
                        ErrorCode::HostBwrapNotFound,
                        "bwrap probe failed",
                    )],
                )
            });
            let u = hu.wait().unwrap_or_else(|_| {
                (
                    None,
                    vec![HostProblem::new(
                        ErrorCode::HostUmuNotFound,
                        "umu probe failed",
                    )],
                )
            });
            let g = hg.wait().unwrap_or_default();
            (b, u, g)
        }
        None => (bwrap_job(), umu_job(), gpu_job()),
    };
    facts.bwrap = bwrap;
    facts.umu = umu;
    facts.gpu = gpu;
    facts.problems.append(&mut bwrap_problems);
    facts.problems.append(&mut umu_problems);

    facts.etc_links = probe_etc_links(fs);
    facts.display = probe_display(fs, &deps.env, &mut facts.notes);
    facts.audio = probe_audio(fs, &deps.env, &mut facts.notes);
    facts.input = probe_input(fs, &*deps.access);
    facts.sysfs = probe_sysfs(fs);
    facts.fs = probe_store_fs(fs, &deps.paths);

    if !facts.display.any() {
        facts.problems.push(HostProblem::new(
            ErrorCode::HostNoDisplay,
            "neither a Wayland socket nor an X11 socket was found",
        ));
    }
    if !facts.gpu.render_nodes.iter().any(|n| n.accessible) {
        facts.problems.push(HostProblem::new(
            ErrorCode::HostNoGpu,
            "no accessible /dev/dri/renderD* node",
        ));
    }
    if deps.env.uid() == 0 {
        facts.problems.push(HostProblem::new(
            ErrorCode::HostRunningAsRoot,
            "running as root",
        ));
    }
    facts
}

/// The store's device, filesystem type and reflink support. The reflink probe needs the store
/// and `session-var` to exist; it is skipped (`None`) otherwise.
pub fn probe_store_fs(fs: &dyn Fs, paths: &AppPaths) -> StoreFsFacts {
    let store = &paths.runtimes_dir;
    let store_device = fs.metadata_follow(store).ok().map(|m| m.device);
    let store_fs_type = MountTable::read(fs)
        .ok()
        .and_then(|t| t.fs_type_of(store).map(str::to_owned));
    let reflink_ok = if fs.exists(store) && fs.exists(&paths.session_var_root) {
        fs.reflink_supported(store, &paths.session_var_root).ok()
    } else {
        None
    };
    StoreFsFacts {
        store_device,
        store_fs_type,
        reflink_ok,
    }
}

// ---- cache ----------------------------------------------------------------------------------

/// `stat` identity of a binary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStamp {
    /// Path.
    pub path: PathBuf,
    /// Device.
    pub device: u64,
    /// Inode.
    pub inode: u64,
    /// Size.
    pub size: u64,
    /// Modification time.
    pub mtime_ns: i64,
}

impl FileStamp {
    /// Stamp `path` (following symlinks); `None` when it does not exist.
    pub fn of(fs: &dyn Fs, path: &Path) -> Option<Self> {
        let md = fs.metadata_follow(path).ok()?;
        Some(Self {
            path: path.to_path_buf(),
            device: md.device,
            inode: md.inode,
            size: md.size,
            mtime_ns: md.mtime_ns,
        })
    }
}

/// What a cached document is valid for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheKey {
    /// [`HOST_FACTS_FORMAT`].
    pub format: u32,
    /// EasyPlay version.
    pub app_version: String,
    /// `/proc/sys/kernel/random/boot_id`.
    pub boot_id: String,
    /// `paths.umu_run`.
    pub umu_override: Option<PathBuf>,
    /// `stat` of the probed bwrap.
    pub bwrap: Option<FileStamp>,
    /// `stat` of the probed umu-run (and of its real path when it is a symlink).
    pub umu: Option<FileStamp>,
}

/// The cache file document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedFacts {
    /// Validity key.
    pub key: CacheKey,
    /// The facts.
    pub facts: HostFacts,
}

/// Read the boot ID.
pub fn boot_id(fs: &dyn Fs) -> String {
    fs.read_follow(Path::new("/proc/sys/kernel/random/boot_id"), 128)
        .map(|b| String::from_utf8_lossy(&b).trim().to_owned())
        .unwrap_or_default()
}

/// The key `facts` would be cached under now.
pub fn current_key(fs: &dyn Fs, facts: &HostFacts, umu_override: Option<&Path>) -> CacheKey {
    CacheKey {
        format: HOST_FACTS_FORMAT,
        app_version: APP_VERSION.to_owned(),
        boot_id: boot_id(fs),
        umu_override: umu_override.map(Path::to_path_buf),
        bwrap: facts
            .bwrap
            .as_ref()
            .and_then(|b| FileStamp::of(fs, &b.path)),
        umu: facts.umu.as_ref().and_then(|u| FileStamp::of(fs, &u.path)),
    }
}

/// Load the cache when its key still matches the machine.
pub fn load_cache(fs: &dyn Fs, file: &Path, umu_override: Option<&Path>) -> Option<HostFacts> {
    let bytes = fs.read(file, 4 * 1024 * 1024).ok()?;
    let cached: CachedFacts = serde_json::from_slice(&bytes).ok()?;
    if cached.facts.format != HOST_FACTS_FORMAT {
        return None;
    }
    let now = current_key(fs, &cached.facts, umu_override);
    (now == cached.key && !now.boot_id.is_empty()).then_some(cached.facts)
}

/// Write the cache (mode 0600).
pub fn save_cache(
    fs: &dyn Fs,
    file: &Path,
    facts: &HostFacts,
    umu_override: Option<&Path>,
) -> Result<(), Error> {
    let doc = CachedFacts {
        key: current_key(fs, facts, umu_override),
        facts: facts.clone(),
    };
    let json = serde_json::to_vec_pretty(&doc).map_err(|e| {
        Error::new(ErrorCode::IntTaskFailed)
            .with("what", "host facts")
            .with_source(e)
    })?;
    if let Some(dir) = file.parent() {
        fs.create_dir_all(dir, 0o700)?;
    }
    fs.atomic_write(file, &json, 0o600)?;
    Ok(())
}

/// Result of the cheap pre-launch check.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Revalidation {
    /// What changed since the probe (empty = facts still valid).
    pub changed: Vec<String>,
}

impl Revalidation {
    /// Nothing changed.
    pub fn is_valid(&self) -> bool {
        self.changed.is_empty()
    }
}

fn socket_exists(fs: &dyn Fs, p: &Path) -> bool {
    fs.metadata_follow(p)
        .is_ok_and(|m| m.file_type == FileType::Socket)
}

/// Re-check binaries and sockets without running anything.
pub fn revalidate(fs: &dyn Fs, facts: &HostFacts, stamp: &CacheKey) -> Revalidation {
    let mut changed = Vec::new();
    if let Some(b) = &facts.bwrap
        && FileStamp::of(fs, &b.path) != stamp.bwrap
    {
        changed.push(format!("bwrap {} changed", b.path.display()));
    }
    if let Some(u) = &facts.umu
        && FileStamp::of(fs, &u.path) != stamp.umu
    {
        changed.push(format!("umu-run {} changed", u.path.display()));
    }
    if let Some(w) = &facts.display.wayland
        && !socket_exists(fs, &w.path)
    {
        changed.push(format!("Wayland socket {} is gone", w.path.display()));
    }
    if let Some(x) = facts.display.x11_bindable()
        && !socket_exists(fs, &x.socket)
    {
        changed.push(format!("X11 socket {} is gone", x.socket.display()));
    }
    if let Some(p) = &facts.audio.pulse_socket
        && !socket_exists(fs, p)
    {
        changed.push(format!("PulseAudio socket {} is gone", p.display()));
    }
    if let Some(p) = &facts.audio.pipewire_socket
        && !socket_exists(fs, p)
    {
        changed.push(format!("PipeWire socket {} is gone", p.display()));
    }
    Revalidation { changed }
}

/// The seam for host facts (ARCHITECTURE §7).
pub trait HostProbe: Send + Sync {
    /// The current facts: cached when valid, probed otherwise.
    fn facts(&self) -> Arc<HostFacts>;
    /// Probe again, ignoring the cache.
    fn refresh(&self) -> Arc<HostFacts>;
    /// Cheap pre-launch check; on change the caller calls [`HostProbe::refresh`].
    fn revalidate(&self) -> Revalidation;
    /// The runner the probe starts its test commands with (the Doctor's D-10 self-test uses
    /// it too); `None` for probes that run nothing.
    fn runner(&self) -> Option<Arc<dyn CommandRunner>> {
        None
    }
}

/// The real probe with its cache.
pub struct LiveHostProbe {
    deps: ProbeDeps,
    current: Mutex<Option<(Arc<HostFacts>, CacheKey)>>,
}

impl LiveHostProbe {
    /// A probe; nothing runs until [`HostProbe::facts`].
    pub fn new(deps: ProbeDeps) -> Self {
        Self {
            deps,
            current: Mutex::new(None),
        }
    }

    fn store(&self, facts: HostFacts) -> Arc<HostFacts> {
        let fs = &*self.deps.fs;
        let key = current_key(fs, &facts, self.deps.umu_override.as_deref());
        if let Err(e) = save_cache(
            fs,
            &self.deps.paths.host_facts_file,
            &facts,
            self.deps.umu_override.as_deref(),
        ) {
            tracing::warn!(code = %e.code, "could not write the host facts cache");
        }
        let facts = Arc::new(facts);
        *self.current.lock() = Some((Arc::clone(&facts), key));
        facts
    }
}

impl HostProbe for LiveHostProbe {
    fn facts(&self) -> Arc<HostFacts> {
        if let Some((f, _)) = &*self.current.lock() {
            return Arc::clone(f);
        }
        let fs = &*self.deps.fs;
        if let Some(facts) = load_cache(
            fs,
            &self.deps.paths.host_facts_file,
            self.deps.umu_override.as_deref(),
        ) {
            let key = current_key(fs, &facts, self.deps.umu_override.as_deref());
            let facts = Arc::new(facts);
            *self.current.lock() = Some((Arc::clone(&facts), key));
            return facts;
        }
        self.refresh()
    }

    fn refresh(&self) -> Arc<HostFacts> {
        let facts = probe_all(&self.deps);
        self.store(facts)
    }

    fn runner(&self) -> Option<Arc<dyn CommandRunner>> {
        Some(Arc::clone(&self.deps.runner))
    }

    fn revalidate(&self) -> Revalidation {
        let current = self.current.lock().clone();
        match current {
            Some((facts, key)) => revalidate(&*self.deps.fs, &facts, &key),
            None => Revalidation {
                changed: vec!["not probed yet".into()],
            },
        }
    }
}

/// A fixed set of facts (tests and `--json` replays).
pub struct StaticHostFacts(pub Arc<HostFacts>);

impl HostProbe for StaticHostFacts {
    fn facts(&self) -> Arc<HostFacts> {
        Arc::clone(&self.0)
    }
    fn refresh(&self) -> Arc<HostFacts> {
        Arc::clone(&self.0)
    }
    fn revalidate(&self) -> Revalidation {
        Revalidation::default()
    }
}
