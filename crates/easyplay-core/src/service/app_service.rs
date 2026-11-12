//! The AppService (SVC-002, SVC-009): one thread, one command loop, all state transitions.
//!
//! It owns the read-write [`Db`] (inside [`Library`]), the library index, the session
//! registry (as the [`ActiveSessions`] snapshot), the preferences, the host facts and the
//! parked flows. Anything that could take longer than a few milliseconds runs on the job
//! pool; its result re-enters the loop as an [`Internal`] message. Snapshots are published
//! to the [`SnapshotHub`]; every publish and event calls the injected waker
//! (`egui::Context::request_repaint` in the GUI).
//!
//! Start-up (PERFORMANCE §3): open and migrate the database, recover sessions left open,
//! load the library and publish it, retry interrupted removals, sweep orphans, rebuild the
//! registry and start watching open sessions; then, on the job pool, probe the host, scan
//! the compatibility layers and run the maintenance job (`flows::maintenance`).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use super::command::{Command, OpenOrigin};
use super::event::{Event, Notice};
use super::flows::artwork::ArtFetched;
use super::flows::launch::LaunchDone;
use super::flows::open::Opened;
use super::prompts::Prompts;
use super::session_watch::{SessionPoll, SessionWatcher, WatchMsg};
use super::snapshot::{ActiveSessions, SnapshotHub};
use crate::artwork::http::HttpClient;
use crate::artwork::steam::{AppIdFile, AttemptTracker};
use crate::artwork::{ArtIndex, IconSource, ImageCodec};
use crate::clock::Clock;
use crate::config::{LoadOptions, Preferences};
use crate::error::catalog::{self, DescribeCtx};
use crate::error::{Error, ErrorCode, UserError};
use crate::fsx::{Fs, MountTable};
use crate::host::{HostFacts, HostProbe};
use crate::identity::SharedLocations;
use crate::identity::pe::PeInspector;
use crate::ids::{GameId, LaunchId, SessionId};
use crate::jobs::{JobLabel, JobPool, JobPriority};
use crate::launch::{HostEnv, PrefixLocks};
use crate::library::draft::UmuTitleMatch;
use crate::library::service::HostPolicyCheck;
use crate::library::{DefaultRuntimeInfo, GameDraft, Library, LibraryDeps, PolicyCheck};
use crate::model::{Activity, EnvVar, Game, UserDir};
use crate::paths::AppPaths;
use crate::procfs::ProcSource;
use crate::runtime::{RuntimeCatalog, RuntimeService};
use crate::sandbox::PolicyIssue;
use crate::session::{self, RecoveryOptions, SessionLauncher};
use crate::storage::{
    ArtworkRepo, Db, GameRepo, KvRepo, MetaKey, MigrateOptions, MigrationOutcome,
};

/// Commands slower than this are logged in debug builds (ARCHITECTURE §5).
pub const COMMAND_BUDGET_MS: u128 = 5;

/// Repaint request for the UI.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Process-wide planning inputs, owned (the borrowed [`HostEnv`] is built per launch).
#[derive(Clone, Debug)]
pub struct LaunchEnv {
    /// User ID.
    pub uid: u32,
    /// Primary group ID.
    pub gid: u32,
    /// User name.
    pub user_name: String,
    /// Primary group name.
    pub group_name: String,
    /// Real home.
    pub home: PathBuf,
    /// The launcher's environment.
    pub host_env: BTreeMap<String, String>,
    /// Host path of `easyplay-init`.
    pub init_binary: PathBuf,
    /// Mount table.
    pub mounts: MountTable,
    /// XDG user directories.
    pub user_dirs: Vec<(UserDir, PathBuf)>,
    /// A seccomp program can be built.
    pub seccomp_available: bool,
    /// Global user variables (preferences).
    pub global_env: Vec<EnvVar>,
}

impl LaunchEnv {
    /// The borrowed planning inputs.
    pub fn host_env<'a>(
        &'a self,
        paths: &'a AppPaths,
        install_id: &'a str,
        other_game_roots: &'a [PathBuf],
        fs: &'a dyn Fs,
    ) -> HostEnv<'a> {
        HostEnv {
            uid: self.uid,
            gid: self.gid,
            user_name: &self.user_name,
            group_name: &self.group_name,
            home: &self.home,
            paths,
            host_env: &self.host_env,
            install_id,
            init_binary: &self.init_binary,
            developer_mode: false,
            seccomp_available: self.seccomp_available,
            verbose_log: false,
            global_env: &self.global_env,
            user_dirs: &self.user_dirs,
            other_game_roots,
            mounts: &self.mounts,
            fs,
        }
    }
}

/// Every seam the service uses, injected at construction (ARCHITECTURE §7).
pub struct ServiceDeps {
    /// Filesystem.
    pub fs: Arc<dyn Fs>,
    /// Time.
    pub clock: Arc<dyn Clock>,
    /// Locations.
    pub paths: AppPaths,
    /// Job pool.
    pub jobs: Arc<JobPool>,
    /// `/proc`.
    pub proc: Arc<dyn ProcSource>,
    /// Starts and stops supervisors.
    pub launcher: Arc<dyn SessionLauncher>,
    /// Prefix lock probe.
    pub locks: Arc<dyn PrefixLocks>,
    /// Compatibility layers.
    pub runtimes: Arc<dyn RuntimeService>,
    /// Host facts.
    pub host: Arc<dyn HostProbe>,
    /// PE reader.
    pub pe: Arc<dyn PeInspector>,
    /// Icon extraction for drafts.
    pub icons: Arc<dyn IconSource>,
    /// Image codec; `None` = no artwork is stored.
    pub codec: Option<Arc<dyn ImageCodec>>,
    /// Planning inputs.
    pub launch_env: LaunchEnv,
    /// Preferences as loaded.
    pub prefs: Preferences,
    /// Desktop integration (handler, shortcuts, opening folders); `None` disables it.
    pub desktop: Option<Arc<crate::desktop::DesktopServices>>,
    /// The HTTP client for Steam artwork (used only with `artwork.online = "steam"`).
    pub http: Option<Arc<HttpClient>>,
    /// `steam_appid.txt` reader.
    pub app_id_file: Arc<dyn AppIdFile>,
    /// Hash → original, shared with the UI's thumbnail service.
    pub art_index: Arc<ArtIndex>,
    /// Shared locations (game-root rules).
    pub shared: SharedLocations,
    /// Policy validation; `None` = validate against the host facts once probed.
    pub policy_check: Option<Arc<dyn PolicyCheck>>,
    /// umu-database title match for drafts.
    pub umu_titles: Arc<dyn UmuTitleMatch>,
    /// Run the session watcher thread (tests drive polls by hand).
    pub watch_sessions: bool,
}

/// Messages into the loop: commands and job completions.
pub(crate) enum Msg {
    Cmd(Command),
    Internal(Internal),
}

/// Job completions and watcher reports (the "internal commands" of SVC-002).
pub(crate) enum Internal {
    Opened(Box<Opened>),
    Drafted {
        result: Result<Box<GameDraft>, Error>,
        origin: OpenOrigin,
    },
    LaunchState {
        game: GameId,
        launch: LaunchId,
        state: crate::launch::LaunchState,
    },
    LaunchDone(Box<LaunchDone>),
    HostFacts(Arc<HostFacts>),
    Runtimes(Arc<RuntimeCatalog>),
    Sessions(SessionPoll),
    Art(Box<ArtFetched>),
    Adopted(Result<crate::runtime::AdoptReport, Error>),
    Flow(Box<super::flows::FlowDone>),
    Desktop(Box<super::flows::desktop::DesktopDone>),
}

/// The handle the UI (and the IPC listener) holds.
pub struct ServiceHandle {
    tx: Sender<Msg>,
    /// The snapshots.
    pub hub: Arc<SnapshotHub>,
    /// Policy explanations for the UI thread (`Backend::assess`).
    pub explainer: Arc<super::explainer::Explainer>,
    waker: Arc<Mutex<Option<Waker>>>,
    thread: Option<JoinHandle<()>>,
}

impl ServiceHandle {
    /// Queue a command (never blocks).
    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(Msg::Cmd(cmd));
    }

    /// A cloneable sender of commands (the instance-socket listener).
    pub fn sender(&self) -> CommandSender {
        CommandSender {
            tx: self.tx.clone(),
        }
    }

    /// Install the repaint waker.
    pub fn set_waker(&self, waker: Waker) {
        *self.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(waker);
    }

    /// Explain `policy` as it would run for `game`, or for a draft at `root` running `exe`
    /// (pure apart from `stat` calls of the policy's paths; safe on the UI thread).
    pub fn explain_policy(
        &self,
        game: Option<GameId>,
        root: Option<&std::path::Path>,
        exe: Option<&std::path::Path>,
        policy: &crate::model::SandboxPolicy,
    ) -> crate::sandbox::explain::PolicyExplanation {
        self.explainer.explain(&self.hub, game, root, exe, policy)
    }

    /// Send `Shutdown` and wait for the thread (games keep running).
    pub fn shutdown(mut self) {
        self.send(Command::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ServiceHandle {
    fn drop(&mut self) {
        if let Some(t) = self.thread.take() {
            let _ = self.tx.send(Msg::Cmd(Command::Shutdown));
            let _ = t.join();
        }
    }
}

/// Queues commands from another thread (never blocks).
#[derive(Clone)]
pub struct CommandSender {
    tx: Sender<Msg>,
}

impl CommandSender {
    /// Queue a command.
    pub fn send(&self, cmd: Command) {
        let _ = self.tx.send(Msg::Cmd(cmd));
    }
}

impl std::fmt::Debug for CommandSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CommandSender")
    }
}

/// A host-dependent policy check that becomes real once the host facts arrive.
pub(crate) struct HostCheckCell {
    inner: Mutex<Option<Arc<dyn PolicyCheck>>>,
}

impl PolicyCheck for HostCheckCell {
    fn issues(&self, game: &Game, other_roots: &[PathBuf]) -> Vec<PolicyIssue> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        inner
            .map(|c| c.issues(game, other_roots))
            .unwrap_or_default()
    }
}

/// The service state on its thread.
pub struct AppService {
    pub(super) deps: ServiceDeps,
    pub(super) lib: Library,
    pub(super) hub: Arc<SnapshotHub>,
    pub(super) prefs: Preferences,
    pub(super) host: Option<Arc<HostFacts>>,
    pub(super) prompts: Prompts,
    pub(super) events: Sender<Event>,
    pub(super) tx: Sender<Msg>,
    waker: Arc<Mutex<Option<Waker>>>,
    pub(super) watcher: Option<SessionWatcher>,
    pub(super) check: Arc<HostCheckCell>,
    pub(super) install_id: String,
    /// Open sessions by ID (registry; the snapshot is derived from it).
    pub(super) sessions: BTreeMap<SessionId, crate::model::Session>,
    /// Launches handed off, by session: (game, launch).
    pub(super) launches: HashMap<SessionId, (GameId, LaunchId)>,
    /// Games whose launch job is running.
    pub(super) launching: HashMap<GameId, LaunchId>,
    /// Removals waiting for a session to end.
    pub(super) remove_after_stop: HashMap<GameId, crate::library::RemoveOptions>,
    /// Drafts waiting for the setup dialog (one shown at a time).
    pub(super) setup_queue: VecDeque<(Box<GameDraft>, OpenOrigin)>,
    pub(super) setup_open: bool,
    pub(super) art_tracker: Arc<AttemptTracker>,
    /// State of the tool, query, maintenance and cancellation flows.
    pub(super) flow: super::flows::FlowState,
    sessions_rev: u64,
    stopping: bool,
    headless_rx: Option<Receiver<Msg>>,
}

/// Open (create, migrate) the read-write library.
pub fn open_db(fs: &dyn Fs, paths: &AppPaths, clock: &dyn Clock) -> Result<Db, Error> {
    fs.create_dir_all(&paths.data_dir, 0o700)?;
    let mut db = Db::open_rw(&paths.db_file)?;
    let opts = MigrateOptions {
        backups_dir: Some(paths.backups_dir.clone()),
        now: clock.now(),
    };
    match db.migrate(&opts)? {
        MigrationOutcome::RefusedNewer { .. } => Err(Error::new(ErrorCode::DbNewerVersion)),
        _ => Ok(db),
    }
}

impl AppService {
    /// Run the start-up sequence on the calling thread, then move the loop to its own
    /// thread. Returns the handle and the event receiver.
    pub fn start(deps: ServiceDeps) -> Result<(ServiceHandle, Receiver<Event>), Error> {
        let (tx, rx) = channel::<Msg>();
        let (ev_tx, ev_rx) = channel::<Event>();
        let waker: Arc<Mutex<Option<Waker>>> = Arc::new(Mutex::new(None));
        let mut svc = Self::open(deps, tx.clone(), ev_tx, Arc::clone(&waker))?;
        let hub = Arc::clone(&svc.hub);
        let explainer = Arc::new(super::explainer::Explainer {
            fs: Arc::clone(&svc.deps.fs),
            paths: svc.deps.paths.clone(),
            env: svc.deps.launch_env.clone(),
        });
        let thread = std::thread::Builder::new()
            .name("app-service".into())
            .spawn(move || {
                svc.after_start();
                svc.run(&rx);
            })
            .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with_source(e))?;
        Ok((
            ServiceHandle {
                tx,
                hub,
                explainer,
                waker,
                thread: Some(thread),
            },
            ev_rx,
        ))
    }

    fn open(
        deps: ServiceDeps,
        tx: Sender<Msg>,
        events: Sender<Event>,
        waker: Arc<Mutex<Option<Waker>>>,
    ) -> Result<Self, Error> {
        let mut db = open_db(&*deps.fs, &deps.paths, &*deps.clock)?;
        // Recovery before anything reads sessions (PROCESS_LIFECYCLE §9).
        match session::recover(
            &mut db,
            &*deps.proc,
            &*deps.fs,
            &deps.paths,
            &*deps.clock,
            &RecoveryOptions::default(),
        ) {
            Ok(r) => {
                for s in r {
                    tracing::info!(session = %s.id, reason = s.reason.as_str(), "recovered");
                }
            }
            Err(e) => tracing::warn!(error = %e, "session recovery failed"),
        }
        let install_id = db
            .meta_get(MetaKey::InstallId)
            .ok()
            .flatten()
            .filter(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
            .unwrap_or_else(|| "0".repeat(32));
        let check = Arc::new(HostCheckCell {
            inner: Mutex::new(deps.policy_check.clone()),
        });
        let lib_deps = LibraryDeps {
            fs: Arc::clone(&deps.fs),
            clock: Arc::clone(&deps.clock),
            paths: deps.paths.clone(),
            runtime: Arc::new(DefaultRuntimeInfo),
            check: Arc::clone(&check) as Arc<dyn PolicyCheck>,
            pe: Arc::clone(&deps.pe),
            codec: deps.codec.clone(),
            mounts: deps.launch_env.mounts.clone(),
            shared: deps.shared.clone(),
            overlay_supported: false,
        };
        let lib = Library::open(db, lib_deps)?;
        let prefs = deps.prefs.clone();
        let hub = Arc::new(SnapshotHub::new((*lib.snapshot()).clone(), prefs.clone()));
        Ok(Self {
            deps,
            lib,
            hub,
            prefs,
            host: None,
            prompts: Prompts::default(),
            events,
            tx,
            waker,
            watcher: None,
            check,
            install_id,
            sessions: BTreeMap::new(),
            launches: HashMap::new(),
            launching: HashMap::new(),
            remove_after_stop: HashMap::new(),
            setup_queue: VecDeque::new(),
            setup_open: false,
            art_tracker: Arc::new(AttemptTracker::new()),
            flow: super::flows::FlowState::default(),
            sessions_rev: 0,
            stopping: false,
            headless_rx: None,
        })
    }

    /// The rest of the start-up sequence (on the service thread).
    fn after_start(&mut self) {
        let keep_history = self.prefs.statistics.keep_history_of_removed;
        if let Err(e) = self
            .lib
            .retry_removals(keep_history, &crate::library::remove::NoHooks)
        {
            tracing::warn!(error = %e, "retrying removals failed");
        }
        match self.lib.sweep_orphans() {
            Ok(r) if !r.removed.is_empty() || !r.listed.is_empty() => {
                tracing::info!(
                    removed = r.removed.len(),
                    listed = r.listed.len(),
                    "orphan sweep"
                );
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "orphan sweep failed"),
        }
        self.refresh_art_index();
        self.publish_library();
        self.refresh_accents(None);
        // Sessions open at start: watch them.
        if self.deps.watch_sessions {
            let tx = self.tx.clone();
            match SessionWatcher::spawn(self.deps.paths.db_file.clone(), move |p| {
                let _ = tx.send(Msg::Internal(Internal::Sessions(p)));
            }) {
                Ok(w) => self.watcher = Some(w),
                Err(e) => tracing::error!(error = %e, "session watcher not started"),
            }
        }
        match session::Registry::rebuild(self.lib.db_mut(), &*self.deps.proc) {
            Ok(reg) => {
                for a in reg.active() {
                    self.sessions.insert(a.session.id, a.session.clone());
                    self.watch(a.session.id);
                }
                self.publish_sessions();
            }
            Err(e) => tracing::warn!(error = %e, "registry rebuild failed"),
        }
        self.probe_host(false);
        self.scan_runtimes();
        self.start_maintenance();
        self.refresh_integration();
    }

    fn run(&mut self, rx: &Receiver<Msg>) {
        while let Ok(msg) = rx.recv() {
            let started = self.deps.clock.mono();
            let name = match &msg {
                Msg::Cmd(c) => c.name(),
                Msg::Internal(_) => "internal",
            };
            match msg {
                Msg::Cmd(c) => self.handle(c),
                Msg::Internal(i) => self.handle_internal(i),
            }
            let ms = self
                .deps
                .clock
                .mono()
                .saturating_duration_since(started)
                .as_millis();
            if cfg!(debug_assertions) && ms > COMMAND_BUDGET_MS {
                tracing::debug!(command = name, ms, "command exceeded its time budget");
            }
            if self.stopping {
                break;
            }
        }
        let cancelled = self.prompts.cancel_all();
        if cancelled > 0 {
            tracing::info!(cancelled, "parked flows cancelled at shutdown");
        }
        self.watcher = None;
        self.emit(Event::QuitRequested);
    }

    /// Handle one command (also used by tests and the headless CLI through
    /// [`AppService::open_headless`]).
    pub fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::OpenPaths { paths, origin } => self.open_paths(paths, origin),
            Command::CreateGame { draft, then } => self.create_game(*draft, then),
            Command::DiscardDraft => self.setup_closed(),
            Command::RemoveGame { id, options } => self.remove_game(id, options),
            Command::SetFavorite { id, favorite } => {
                let r = self.lib.set_favorite(&[id], favorite).map(|_| ());
                self.after_library_edit(r);
            }
            Command::SetHidden { id, hidden } => {
                let r = self.lib.set_hidden(&[id], hidden).map(|_| ());
                self.after_library_edit(r);
            }
            Command::SetTags { id, tags } => {
                let r = self.lib.set_tags(id, &tags).map(|_| ());
                self.after_library_edit(r);
            }
            Command::UpdateGame { id, edits } => self.update_game(id, edits),
            Command::LoadGame { id, reply_to } => self.load_game(id, reply_to),
            Command::QueryStats { request, reply_to } => self.query_stats(&request, reply_to),
            Command::ClearStatistics => self.clear_statistics(None),
            Command::ResetGameStatistics { id } => self.clear_statistics(Some(id)),
            Command::RemoveSession { id } => self.remove_session(id),
            Command::AddExecutable { id, path, role } => self.add_program(id, &path, role),
            Command::LocateExecutable { id, exe, new_path } => {
                self.locate_program(id, exe, &new_path);
            }
            Command::ScanFolder { folder, reply_to } => self.scan_folder(folder, reply_to),
            Command::MeasurePrefix { id, reply_to } => self.measure_prefix(id, reply_to),
            Command::CancelLaunch { id } => self.cancel_launch(id),
            Command::RunTool { id, tool } => self.run_tool(id, tool, None),
            Command::ImportArtwork { id, slot, source } => self.import_artwork(id, slot, source),
            Command::FetchOnlineArtwork { id } => self.fetch_online_artwork(id, true),
            Command::Launch { id, exe, origin } => self.start_launch(id, exe, origin),
            Command::StopSession { session, mode } => self.stop_session(session, mode),
            Command::AnswerPrompt { prompt, answer } => self.answer_prompt(prompt, answer),
            Command::ResetPrefix { id, options } => self.reset_prefix(id, options),
            Command::AdoptRuntime { variant } => self.adopt_runtime(variant),
            Command::RefreshRuntimes => self.scan_runtimes(),
            Command::ProvisionRuntime { .. }
            | Command::RemoveRuntime { .. }
            | Command::VerifyRuntime => self.fail(Error::not_available("M7")),
            Command::SetPreference(patch) => self.set_preference(&patch),
            Command::RefreshHostFacts => self.probe_host(true),
            Command::SetDefaultHandler { enable } => self.set_default_handler(enable),
            Command::SetShortcut { id, enable } => self.set_shortcut(id, enable),
            Command::OpenFolder(target) => self.open_folder(target),
            Command::ViewLog { session, game } => self.view_log(session, game),
            Command::Shutdown => self.stopping = true,
        }
    }

    pub(super) fn handle_internal(&mut self, i: Internal) {
        match i {
            Internal::Opened(o) => self.on_opened(*o),
            Internal::Drafted { result, origin } => self.on_drafted(result, origin),
            Internal::LaunchState {
                game,
                launch,
                state,
            } => self.on_launch_state(game, launch, state),
            Internal::LaunchDone(d) => self.on_launch_done(*d),
            Internal::HostFacts(f) => self.on_host_facts(f),
            Internal::Runtimes(c) => {
                self.hub.runtimes.store(c);
                self.emit(Event::RuntimesChanged);
            }
            Internal::Sessions(p) => self.on_sessions(p),
            Internal::Art(a) => self.on_art_fetched(*a),
            Internal::Adopted(r) => self.on_adopted(r),
            Internal::Flow(f) => self.on_flow_done(*f),
            Internal::Desktop(d) => self.on_desktop(*d),
        }
    }

    // ---- plumbing -----------------------------------------------------------------------

    /// Post a message to the loop from a job.
    pub(super) fn poster(&self) -> impl Fn(Internal) + Send + Sync + 'static {
        let tx = Mutex::new(self.tx.clone());
        move |i| {
            let _ = tx
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .send(Msg::Internal(i));
        }
    }

    pub(super) fn wake(&self) {
        if let Some(w) = self
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            w();
        }
    }

    /// Send an event and wake the UI.
    pub(super) fn emit(&self, ev: Event) {
        let _ = self.events.send(ev);
        self.wake();
    }

    /// Describe an error for the user.
    pub(super) fn describe(&self, err: &Error, game: Option<GameId>) -> UserError {
        let redact = |s: &str| s.to_owned();
        let ctx = DescribeCtx {
            game,
            session: None,
            redact: &redact,
        };
        catalog::describe(err, &ctx)
    }

    /// `Event::Failure`.
    pub(super) fn fail(&self, err: Error) {
        tracing::info!(code = err.code.as_str(), "{err}");
        let u = self.describe(&err, None);
        self.emit(Event::Failure(Box::new(u)));
    }

    pub(super) fn notice(&self, n: Notice) {
        self.emit(Event::Notice(n));
    }

    /// Publish the library snapshot and announce it.
    pub(super) fn publish_library(&self) {
        self.hub.library.store(self.lib.snapshot());
        self.emit(Event::LibraryChanged);
        self.sync_shortcuts();
    }

    fn after_library_edit(&mut self, r: Result<(), Error>) {
        match r {
            Ok(()) => self.publish_library(),
            Err(e) => self.fail(e),
        }
    }

    pub(super) fn publish_sessions(&mut self) {
        self.sessions_rev += 1;
        let snap = ActiveSessions {
            revision: self.sessions_rev,
            sessions: self.sessions.values().cloned().collect(),
        };
        self.hub.sessions.store(Arc::new(snap));
        self.emit(Event::SessionsChanged);
    }

    pub(super) fn watch(&self, id: SessionId) {
        if let Some(w) = &self.watcher {
            w.send(WatchMsg::Track(id));
        }
    }

    pub(super) fn poke_watcher(&self) {
        if let Some(w) = &self.watcher {
            w.send(WatchMsg::Poke);
        }
    }

    /// Rebuild the artwork index from the rows (start-up, after imports).
    pub(super) fn refresh_art_index(&mut self) {
        match self.lib.db_mut().load_library() {
            Ok(rows) => self
                .deps
                .art_index
                .replace_all(&self.deps.paths, &rows.artwork),
            Err(e) => tracing::warn!(error = %e, "artwork index not refreshed"),
        }
    }

    /// Add the artwork rows of one game to the index (and work out its accent again).
    pub(super) fn index_game_art(&mut self, id: GameId) {
        if let Ok(rows) = self.lib.db_mut().artwork_of(id) {
            for r in &rows {
                self.deps.art_index.insert(&self.deps.paths, r);
            }
        }
        self.refresh_accents(Some(&[id]));
    }

    pub(super) fn set_activity(&mut self, id: GameId, a: Activity) {
        self.lib.set_activity(id, a);
    }

    // ---- game page (UI seam) ------------------------------------------------------------

    fn update_game(&mut self, id: GameId, edits: Vec<super::command::GameEdit>) {
        use super::command::GameEdit as E;
        let mut patch = crate::storage::GamePatch::default();
        let mut program_edits = Vec::new();
        for e in edits {
            match e {
                E::Name(n) => {
                    patch.name = Some(n);
                    patch.name_is_user_set = Some(true);
                }
                E::Notes(n) => patch.notes = Some(n),
                E::Policy(p) => {
                    let mut p = *p;
                    p.preset = crate::sandbox::classify(&p);
                    patch.policy = Some(p);
                }
                E::Launch(l) => patch.launch = Some(*l),
                E::Runtime { reference, follow } => {
                    patch.runtime = Some(reference);
                    patch.runtime_follow_updates = Some(follow);
                }
                E::UmuId(u) => patch.umu_id = Some(u),
                E::SteamAppId(a) => {
                    patch.steam_app_id = Some(a);
                    patch.steam_app_id_is_user_set = Some(a.is_some());
                }
                E::ResetName => match self.lib.game(id) {
                    Ok(g) => {
                        let Some(exe) = g.primary() else { continue };
                        let n = crate::identity::naming::derive_name(
                            &exe.pe,
                            g.root.as_deref(),
                            &exe.canonical_path,
                        );
                        patch.sort_name = Some(crate::identity::naming::sort_name(&n));
                        patch.name = Some(n);
                        patch.name_is_user_set = Some(false);
                    }
                    Err(e) => return self.fail(e),
                },
                E::Kind(k) => {
                    let flags = match patch.flags {
                        Some(f) => f,
                        None => match self.lib.game(id) {
                            Ok(g) => g.flags,
                            Err(e) => return self.fail(e),
                        },
                    };
                    patch.flags = Some(crate::library::with_entry_kind(flags, k));
                }
                E::ExeRole(..) | E::ExeLabel(..) | E::ExeArgs(..) | E::RemoveExe(_) => {
                    program_edits.push(e);
                }
                E::Preset(b) => {
                    let base = match patch.policy.take() {
                        Some(p) => p,
                        None => match self.lib.game(id) {
                            Ok(g) => g.policy,
                            Err(e) => return self.fail(e),
                        },
                    };
                    let host = self
                        .host
                        .as_deref()
                        .cloned()
                        .unwrap_or_else(|| HostFacts::empty(self.deps.clock.now()));
                    patch.policy = Some(crate::sandbox::apply_preset(&base, b, &host));
                }
            }
        }
        let mut r = if patch == crate::storage::GamePatch::default() {
            Ok(())
        } else {
            self.lib.update(id, patch).map(|_| ())
        };
        for e in program_edits {
            if r.is_err() {
                break;
            }
            r = match e {
                E::ExeRole(x, role) => self.lib.set_program_role(id, x, role),
                E::ExeLabel(x, label) => self.lib.set_program_label(id, x, label),
                E::ExeArgs(x, args) => self.lib.set_program_args(id, x, args),
                E::RemoveExe(x) => self.lib.remove_program(id, x),
                _ => Ok(()),
            };
        }
        self.after_library_edit(r);
    }

    fn load_game(&mut self, id: GameId, reply_to: crate::ids::QueryId) {
        let result = match self.lib.game(id) {
            Ok(g) => super::event::QueryResult::Game(Box::new(g)),
            Err(e) => super::event::QueryResult::Failed(Box::new(self.describe(&e, Some(id)))),
        };
        self.emit(Event::QueryResult {
            id: reply_to,
            result,
        });
    }

    fn query_stats(&mut self, req: &crate::stats::StatsRequest, reply_to: crate::ids::QueryId) {
        use crate::stats::StatsRequest as R;
        let now = self.deps.clock.now();
        // The UI computes ranges in local time; their end date is "today" for streaks.
        let today = match req {
            R::LibraryOverview { range }
            | R::GameOverview { range, .. }
            | R::TopGames { range, .. }
            | R::WeekdayDistribution { range } => Some(range.to),
            R::SessionHistory { .. } => None,
        }
        .or_else(|| crate::stats::rollup::key_date(crate::stats::rollup::local_day(now, 0)));
        let Some(today) = today else {
            return;
        };
        let week_start = match self.prefs.statistics.week_starts_on {
            crate::config::schema::WeekStart::Sunday => jiff::civil::Weekday::Sunday,
            _ => jiff::civil::Weekday::Monday,
        };
        let ctx = crate::stats::QueryContext { today, week_start };
        // Off the service thread (PERFORMANCE §2): a read-only connection on the job pool.
        let db = Arc::clone(&self.flow.stats_db);
        let file = self.deps.paths.db_file.clone();
        let req = req.clone();
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("stats-query"),
            move |_| {
                let mut guard = db.lock().unwrap_or_else(PoisonError::into_inner);
                if guard.is_none() {
                    match crate::storage::ReadDb::open(&file) {
                        Ok(d) => *guard = Some(d),
                        Err(e) => tracing::warn!(error = %e, "statistics connection failed"),
                    }
                }
                let result = match guard.as_mut() {
                    Some(d) => crate::stats::queries::run(d, &ctx, &req).map_err(Error::from),
                    None => Err(Error::new(ErrorCode::DbBusy)),
                };
                let result = match result {
                    Ok(r) => super::event::QueryResult::Stats(r),
                    Err(e) => {
                        let redact = |s: &str| s.to_owned();
                        let u = catalog::describe(
                            &e,
                            &DescribeCtx {
                                game: None,
                                session: None,
                                redact: &redact,
                            },
                        );
                        super::event::QueryResult::Failed(Box::new(u))
                    }
                };
                post(Internal::Flow(Box::new(super::flows::FlowDone::Query {
                    id: reply_to,
                    result,
                })));
            },
        );
    }

    // ---- host and preferences (SVC-009) -----------------------------------------------

    fn probe_host(&self, fresh: bool) {
        let host = Arc::clone(&self.deps.host);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("host-probe"),
            move |_| {
                let f = if fresh { host.refresh() } else { host.facts() };
                post(Internal::HostFacts(f));
            },
        );
    }

    fn on_host_facts(&mut self, f: Arc<HostFacts>) {
        if self.deps.policy_check.is_none() {
            let e = &self.deps.launch_env;
            let c = HostPolicyCheck {
                fs: Arc::clone(&self.deps.fs),
                mounts: e.mounts.clone(),
                paths: self.deps.paths.clone(),
                host: (*f).clone(),
                home: e.home.clone(),
                uid: e.uid,
                user_name: e.user_name.clone(),
                user_dirs: e.user_dirs.clone(),
                developer_mode: false,
                global_env: e.global_env.clone(),
            };
            *self
                .check
                .inner
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(c));
        }
        self.hub.host.store(Arc::new(Some((*f).clone())));
        self.host = Some(f);
        self.emit(Event::HostFactsChanged);
    }

    fn set_preference(&mut self, patch: &crate::config::PreferencePatch) {
        let next = match self
            .prefs
            .apply(patch, &*self.deps.fs, &LoadOptions { env_rule: None })
        {
            Ok(p) => p,
            Err(e) => return self.fail(e.into()),
        };
        if let Err(e) = crate::config::save(&*self.deps.fs, &self.deps.paths.config_file, &next) {
            return self.fail(e.into());
        }
        self.prefs = next.clone();
        self.hub.prefs.store(Arc::new(next));
        self.emit(Event::PreferencesChanged);
    }

    // ---- headless use ------------------------------------------------------------------

    /// The service without a thread, for the headless CLI and tests: start-up runs, then
    /// the caller drives [`AppService::handle`] and [`AppService::pump`]. No watcher.
    pub fn open_headless(mut deps: ServiceDeps) -> Result<(Self, Receiver<Event>), Error> {
        deps.watch_sessions = false;
        let (tx, rx) = channel::<Msg>();
        let (ev_tx, ev_rx) = channel::<Event>();
        let mut svc = Self::open(deps, tx, ev_tx, Arc::new(Mutex::new(None)))?;
        svc.headless_rx = Some(rx);
        svc.after_start();
        Ok((svc, ev_rx))
    }

    /// Headless: handle the next internal message (job completion), waiting at most
    /// `timeout`. Returns whether one was handled.
    pub fn pump(&mut self, timeout: std::time::Duration) -> bool {
        let Some(rx) = self.headless_rx.take() else {
            return false;
        };
        let got = match rx.recv_timeout(timeout) {
            Ok(Msg::Internal(i)) => {
                self.handle_internal(i);
                true
            }
            Ok(Msg::Cmd(c)) => {
                self.handle(c);
                true
            }
            Err(_) => false,
        };
        self.headless_rx = Some(rx);
        got
    }

    /// Headless: feed a watcher poll by hand (tests; the CLI waits through the database).
    pub fn poll_sessions_now(&mut self) {
        let mut tracked: std::collections::BTreeSet<SessionId> =
            self.launches.keys().copied().collect();
        if let Some(p) = super::session_watch::poll(self.lib.db_mut(), &mut tracked) {
            self.on_sessions(p);
        }
    }

    /// The current snapshots.
    pub fn hub(&self) -> &Arc<SnapshotHub> {
        &self.hub
    }

    /// The library (headless read access).
    pub fn library(&mut self) -> &mut Library {
        &mut self.lib
    }
}
