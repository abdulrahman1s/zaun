//! Steam games (ADR-0031): the option, scans, Play / Update / Open in Steam, the running
//! state, the Windows-storage actions, and the general commands addressed to a Steam game.
//!
//! - **Scans** run on the job pool (`steam::scan`, read-only) when the option is turned on,
//!   at start, with every "check for missing games" and on request.
//! - **Requests to Steam** are handed to the Steam client of the installation that lists the
//!   game, detached ([`Handoff`]); EasyPlay never waits for, owns or stops what Steam starts.
//! - **Running** is seen through `/proc` (`steam::running_games`) by a small polling thread
//!   that runs only after a Play request or while a Steam game is seen running, so an idle
//!   EasyPlay does no work for Steam.
//! - **Windows storage**: a tool runs as an EasyPlay `Tool` session in the game's
//!   `compatdata/<appid>` with the Proton build that last ran it — inside EasyPlay's
//!   protection (Offline preset), and only while the game is not running.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::super::app_service::{AppService, Internal};
use super::super::command::{Command, FolderRequest, GameEdit, SteamCommand};
use super::super::event::{Event, Notice, QueryResult, SteamDetail};
use super::FlowDone;
use super::desktop::DesktopDone;
use crate::clock::{MonoInstant, Timestamp};
use crate::desktop::KnownPath;
use crate::error::{Error, ErrorCode, Result};
use crate::ids::{ExecutableId, GameId, QueryId, SessionId};
use crate::jobs::{JobLabel, JobPriority};
use crate::launch::{LaunchTarget, PlanRequest, plan_launch_in_prefix};
use crate::library::steam::{is_listed, not_for_steam};
use crate::model::{
    Activity, ExeRole, Executable, Fingerprint, Game, GameFlags, GameSummary, LaunchConfig,
    LaunchStateTag, PeInfo, RuntimeRef, SteamCompat, Tool,
};
use crate::procfs::ProcIdentity;
use crate::runtime::Resolution;
use crate::steam::client::{SteamRequest, command, running_games};
use crate::steam::{Handoff, SteamScan};

/// How often the running state is read while a Steam game is starting or running.
pub const STEAM_POLL: Duration = Duration::from_secs(3);
/// How long a Play request waits to see the game start before it is forgotten.
pub const STEAM_START_WINDOW: Duration = Duration::from_secs(180);

/// The Steam part of the service's state.
#[derive(Default)]
pub(crate) struct SteamState {
    /// Starts the Steam client; `None` = requests to Steam are unavailable.
    pub handoff: Option<Arc<dyn Handoff>>,
    /// The option as last acted on.
    enabled: bool,
    /// A scan job runs.
    scanning: bool,
    /// Another scan was asked for while one ran (`Some(announce)`).
    again: Option<bool>,
    /// Play requests not yet seen running: the end of their window.
    pending: BTreeMap<u32, MonoInstant>,
    /// Games seen running, since.
    running: BTreeMap<u32, Timestamp>,
    /// The polling thread's keep-running flag.
    watcher: Option<Arc<AtomicBool>>,
}

impl SteamState {
    /// Whether Steam runs the game now (as last seen).
    pub fn is_running(&self, app_id: u32) -> bool {
        self.running.contains_key(&app_id)
    }
}

impl Drop for SteamState {
    fn drop(&mut self) {
        if let Some(f) = &self.watcher {
            f.store(false, Ordering::Relaxed);
        }
    }
}

/// Completions of the Steam jobs.
pub(crate) enum SteamDone {
    /// A scan finished.
    Scanned {
        /// The result.
        scan: Box<SteamScan>,
        /// Asked for by the user (a toast follows).
        announce: bool,
    },
    /// Steam games running now.
    Running(BTreeMap<u32, ProcIdentity>),
    /// A request was handed to Steam (or not).
    Asked {
        /// The game.
        game: Option<GameId>,
        /// What.
        request: SteamRequest,
        /// Whether it was handed over.
        result: Result<()>,
    },
}

/// The game a prefix tool runs under: never stored; the Offline preset with the program's
/// folder read-only (the file alone in a shared location such as Downloads), and the Proton
/// build Steam last used.
pub fn tool_game(
    id: GameId,
    name: &str,
    program: &Path,
    root: Option<&Path>,
    proton: &Path,
    now: Timestamp,
) -> Game {
    let mut policy = crate::sandbox::offline();
    policy.game_dir = if root.is_some() {
        crate::model::GameDirAccess::ReadOnly
    } else {
        crate::model::GameDirAccess::FileOnly
    };
    policy.preset = crate::sandbox::classify(&policy);
    Game {
        id,
        name: name.to_owned(),
        sort_name: crate::model::sort_name_of(name),
        name_is_user_set: false,
        root: root.map(Path::to_path_buf),
        added_at: now,
        updated_at: now,
        favorite: false,
        hidden: false,
        notes: String::new(),
        policy,
        policy_updated_at: now,
        launch: LaunchConfig::default(),
        umu_id: None,
        store: None,
        runtime: RuntimeRef::External(proton.to_path_buf()),
        runtime_follow_updates: false,
        runtime_last_resolved: None,
        prefix_tainted_at: None,
        prefix_prepared_with: None,
        flags: GameFlags::empty(),
        last_launched_at: None,
        steam_app_id: id.steam_app_id(),
        steam_app_id_is_user_set: false,
        executables: vec![Executable {
            id: ExecutableId::from_uuid(id.as_uuid()),
            role: ExeRole::Primary,
            path: program.to_path_buf(),
            canonical_path: program.to_path_buf(),
            rel_path: None,
            mount_point: None,
            fingerprint: Fingerprint {
                size: 0,
                mtime_ns: 0,
                quick_hash: [0; 32],
                full_hash: None,
            },
            pe: PeInfo::unknown(),
            args_override: None,
            label: None,
            added_at: now,
        }],
    }
}

/// Adjust a planned Steam prefix tool session: there is no `games` row, so the session is
/// recorded without a game (its name is kept, like a removed game's history) and the
/// prefix is not marked as having run untrusted code (Steam's prefix is not EasyPlay's to
/// track; the game Steam runs there is outside EasyPlay's protection anyway).
pub fn steam_tool_plan(plan: &mut crate::launch::LaunchPlan) {
    plan.session.game_id = None;
    plan.host_actions
        .retain(|a| !matches!(a, crate::sandbox::plan::HostAction::MarkTainted { .. }));
}

impl AppService {
    /// Use `handoff` to reach the Steam client (tests, the headless command line). The GUI's
    /// service uses `session::RealHandoff` whenever desktop integration is on.
    pub fn set_steam_handoff(&mut self, handoff: Arc<dyn Handoff>) {
        self.flow.steam.handoff = Some(handoff);
    }

    /// Start-up: scan when the option is on.
    pub(crate) fn steam_start(&mut self) {
        if self.flow.steam.handoff.is_none() && self.deps.desktop.is_some() {
            self.flow.steam.handoff = Some(Arc::new(crate::session::RealHandoff));
        }
        self.steam_preference_changed();
    }

    /// The preferences changed: scan when the option was turned on, clear the Steam games
    /// when it was turned off.
    pub(crate) fn steam_preference_changed(&mut self) {
        let on = self.prefs.steam.show_games;
        if on == self.flow.steam.enabled {
            return;
        }
        self.flow.steam.enabled = on;
        if on {
            self.steam_scan(false);
        } else {
            let idle: Vec<(GameId, Activity)> = self
                .flow
                .steam
                .pending
                .keys()
                .chain(self.flow.steam.running.keys())
                .map(|a| (GameId::steam(*a), Activity::Idle))
                .collect();
            self.lib.steam_set_running(&idle);
            self.flow.steam.pending.clear();
            self.flow.steam.running.clear();
            self.steam_stop_watch();
            match self.lib.set_steam(None) {
                Ok(()) => self.publish_library(),
                Err(e) => self.fail(e),
            }
        }
    }

    /// Read Steam's files on the job pool (`announce`: a toast with the count follows).
    pub(crate) fn steam_scan(&mut self, announce: bool) {
        if !self.flow.steam.enabled {
            return;
        }
        if self.flow.steam.scanning {
            let a = self.flow.steam.again.unwrap_or(false) || announce;
            self.flow.steam.again = Some(a);
            return;
        }
        self.flow.steam.scanning = true;
        let fs = Arc::clone(&self.deps.fs);
        let home = self.deps.launch_env.home.clone();
        let proc = Arc::clone(&self.deps.proc);
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Background, JobLabel("steam-scan"), move |_| {
                let scan = crate::steam::scan(&*fs, &home);
                let running = running_games(&*proc);
                post(Internal::Flow(Box::new(FlowDone::Steam(
                    SteamDone::Scanned {
                        scan: Box::new(scan),
                        announce,
                    },
                ))));
                post(Internal::Flow(Box::new(FlowDone::Steam(
                    SteamDone::Running(running),
                ))));
            });
    }

    /// A Steam job finished.
    pub(crate) fn on_steam_done(&mut self, done: SteamDone) {
        match done {
            SteamDone::Scanned { scan, announce } => self.on_steam_scanned(*scan, announce),
            SteamDone::Running(r) => self.on_steam_running(&r),
            SteamDone::Asked {
                game,
                request,
                result,
            } => match result {
                Ok(()) => {
                    if let SteamRequest::Run(app) = request {
                        let until = self.deps.clock.mono().saturating_add(STEAM_START_WINDOW);
                        self.flow.steam.pending.insert(app, until);
                        if !self.flow.steam.is_running(app) {
                            self.lib.steam_set_running(&[(
                                GameId::steam(app),
                                Activity::Launching(LaunchStateTag::Starting),
                            )]);
                            self.publish_library();
                        }
                        self.steam_watch();
                    }
                    self.notice(Notice::SteamAsked { game, request });
                }
                Err(e) => {
                    let u = self.describe(&e, game);
                    self.emit(Event::Failure(Box::new(u)));
                }
            },
        }
    }

    fn on_steam_scanned(&mut self, scan: SteamScan, announce: bool) {
        self.flow.steam.scanning = false;
        if let Some(a) = self.flow.steam.again.take() {
            // The files may have changed meanwhile: read them again before publishing.
            self.steam_scan(a || announce);
            return;
        }
        if !self.flow.steam.enabled {
            return;
        }
        if announce && scan.roots.is_empty() {
            self.fail(Error::new(ErrorCode::StmNotFound));
        }
        let count = scan.games.len();
        let found = !scan.roots.is_empty();
        tracing::info!(
            roots = scan.roots.len(),
            games = count,
            skipped = scan.skipped.len(),
            "steam scan"
        );
        let scan = Arc::new(scan);
        match self.lib.set_steam(Some(Arc::clone(&scan))) {
            Ok(()) => {
                self.steam_index_art();
                self.publish_library();
                if announce && found {
                    self.notice(Notice::SteamScanned { games: count });
                }
            }
            Err(e) => self.fail(e),
        }
    }

    /// Point the artwork index at the pictures in Steam's cache (after a scan and after the
    /// index was rebuilt from the database rows).
    pub(crate) fn steam_index_art(&self) {
        let Some(scan) = self.lib.steam_scan() else {
            return;
        };
        for g in &scan.games {
            for a in &g.art {
                self.deps
                    .art_index
                    .insert_path(a.hash, a.path.clone(), a.slot);
            }
        }
    }

    fn on_steam_running(&mut self, now_running: &BTreeMap<u32, ProcIdentity>) {
        if !self.flow.steam.enabled {
            // A poll that finished after the option was turned off.
            self.steam_stop_watch();
            return;
        }
        let now = self.deps.clock.now();
        let mono = self.deps.clock.mono();
        let st = &mut self.flow.steam;
        let mut changes: Vec<(GameId, Activity)> = Vec::new();
        for app in now_running.keys() {
            st.pending.remove(app);
            if !st.running.contains_key(app) {
                st.running.insert(*app, now);
                changes.push((GameId::steam(*app), Activity::Running { since: now }));
            }
        }
        let ended: Vec<u32> = st
            .running
            .keys()
            .filter(|a| !now_running.contains_key(a))
            .copied()
            .collect();
        for app in ended {
            st.running.remove(&app);
            changes.push((GameId::steam(app), Activity::Idle));
        }
        let expired: Vec<u32> = st
            .pending
            .iter()
            .filter(|(_, until)| **until <= mono)
            .map(|(a, _)| *a)
            .collect();
        for app in expired {
            st.pending.remove(&app);
            changes.push((GameId::steam(app), Activity::Idle));
        }
        let idle = st.pending.is_empty() && st.running.is_empty();
        if !changes.is_empty() {
            self.lib.steam_set_running(&changes);
            self.publish_library();
        }
        if idle {
            self.steam_stop_watch();
        } else {
            self.steam_watch();
        }
    }

    /// Poll the running state now (tests and the headless command line; the GUI's service
    /// polls on its own thread).
    pub fn steam_poll_now(&mut self) {
        let r = running_games(&*self.deps.proc);
        self.on_steam_running(&r);
    }

    fn steam_watch(&mut self) {
        if !self.deps.watch_sessions
            || self
                .flow
                .steam
                .watcher
                .as_ref()
                .is_some_and(|f| f.load(Ordering::Relaxed))
        {
            return;
        }
        let flag = Arc::new(AtomicBool::new(true));
        let keep = Arc::clone(&flag);
        let proc = Arc::clone(&self.deps.proc);
        let post = self.poster();
        let spawned = std::thread::Builder::new()
            .name("steam-watch".into())
            .spawn(move || {
                while keep.load(Ordering::Relaxed) {
                    std::thread::sleep(STEAM_POLL);
                    if !keep.load(Ordering::Relaxed) {
                        break;
                    }
                    let r = running_games(&*proc);
                    post(Internal::Flow(Box::new(FlowDone::Steam(
                        SteamDone::Running(r),
                    ))));
                }
            });
        match spawned {
            Ok(_) => self.flow.steam.watcher = Some(flag),
            Err(e) => tracing::warn!(error = %e, "steam watcher not started"),
        }
    }

    fn steam_stop_watch(&mut self) {
        if let Some(f) = self.flow.steam.watcher.take() {
            f.store(false, Ordering::Relaxed);
        }
    }

    // ---- commands ------------------------------------------------------------------------

    /// Handle `cmd` when it is a Steam command or addressed to a Steam game; otherwise give
    /// it back. `RescanMissing` also reads Steam's files again and is given back.
    pub(crate) fn steam_route(&mut self, cmd: Command) -> Option<Command> {
        let steam = |id: &GameId| id.steam_app_id();
        match cmd {
            Command::Steam(c) => {
                self.steam_command(c);
                None
            }
            Command::RescanMissing => {
                self.steam_scan(false);
                Some(Command::RescanMissing)
            }
            Command::SetFavorite { id, favorite } if steam(&id).is_some() => {
                let r = self.lib.steam_set_favorite(id.steam_app_id()?, favorite);
                self.steam_after_edit(r);
                None
            }
            Command::SetHidden { id, hidden } if steam(&id).is_some() => {
                let r = self.lib.steam_set_hidden(id.steam_app_id()?, hidden);
                self.steam_after_edit(r);
                None
            }
            Command::SetTags { id, tags } if steam(&id).is_some() => {
                let r = self.lib.steam_set_tags(id.steam_app_id()?, &tags);
                self.steam_after_edit(r);
                None
            }
            Command::UpdateGame { id, edits } if steam(&id).is_some() => {
                self.steam_update_game(id, edits);
                None
            }
            Command::Launch { id, .. } if steam(&id).is_some() => {
                self.steam_ask(Some(id), SteamRequest::Run(id.steam_app_id()?));
                None
            }
            Command::LoadGame { id, reply_to } if steam(&id).is_some() => {
                self.steam_details(id, reply_to);
                None
            }
            Command::CancelLaunch { id } if steam(&id).is_some() => {
                // Nothing EasyPlay started can be cancelled: Steam owns the launch.
                None
            }
            Command::RunTool { id, tool } if steam(&id).is_some() => {
                self.steam_run_in_prefix(id, tool);
                None
            }
            Command::OpenFolder(FolderRequest::GameFolder(id)) if steam(&id).is_some() => {
                self.steam_open_folder(id, false);
                None
            }
            Command::OpenFolder(FolderRequest::Prefix(id)) if steam(&id).is_some() => {
                self.steam_open_folder(id, true);
                None
            }
            Command::RemoveGame { id, .. }
            | Command::ResetPrefix { id, .. }
            | Command::AboutGame { id, .. }
            | Command::MeasurePrefix { id, .. }
            | Command::SetShortcut { id, .. }
            | Command::ImportArtwork { id, .. }
            | Command::FetchOnlineArtwork { id }
            | Command::FindSteamApp { id, .. }
            | Command::AddExecutable { id, .. }
            | Command::LocateExecutable { id, .. }
            | Command::ResetGameStatistics { id }
            | Command::OpenFolder(FolderRequest::SaveData(id))
                if steam(&id).is_some() =>
            {
                self.fail(not_for_steam("game action"));
                None
            }
            other => Some(other),
        }
    }

    fn steam_after_edit(&mut self, r: Result<()>) {
        match r {
            Ok(()) => self.publish_library(),
            Err(e) => self.fail(e),
        }
    }

    fn steam_update_game(&mut self, id: GameId, edits: Vec<GameEdit>) {
        let Some(app) = id.steam_app_id() else { return };
        for e in edits {
            let r = match e {
                GameEdit::Notes(n) => self.lib.steam_set_notes(app, &n),
                // Name, protection, compatibility layer, launch options and programs are
                // Steam's or do not apply.
                _ => Err(not_for_steam("game edit")),
            };
            if let Err(e) = r {
                return self.fail(e);
            }
        }
        self.publish_library();
    }

    fn steam_command(&mut self, c: SteamCommand) {
        match c {
            SteamCommand::SetEnabled(on) => {
                let patch = crate::config::PreferencePatch::new().set("steam.show_games", on);
                self.handle(Command::SetPreference(patch));
            }
            SteamCommand::Rescan => {
                if self.flow.steam.enabled {
                    self.steam_scan(true);
                } else {
                    self.fail(not_for_steam("scan while the option is off"));
                }
            }
            SteamCommand::Update(id) => self.steam_ask(Some(id), SteamRequest::Downloads),
            SteamCommand::OpenInSteam(id) => match id.steam_app_id() {
                Some(app) => self.steam_ask(Some(id), SteamRequest::Details(app)),
                None => self.fail(Error::new(ErrorCode::StmGameGone)),
            },
            SteamCommand::OpenPrefixFolder(id) => self.steam_open_folder(id, true),
            SteamCommand::RunInPrefix { id, tool } => self.steam_run_in_prefix(id, tool),
            SteamCommand::Details { id, reply_to } => self.steam_details(id, reply_to),
        }
    }

    /// The listed summary of a Steam game, or `EP-STM-002`.
    fn steam_summary(&self, id: GameId) -> Result<GameSummary> {
        let snap = self.lib.snapshot();
        if !is_listed(&snap, id) {
            return Err(crate::library::steam::not_listed(
                id.steam_app_id().unwrap_or(0),
            ));
        }
        snap.get(id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::StmGameGone))
    }

    /// The command that hands `request` to the Steam installation listing `id` (any
    /// installation for the downloads page).
    fn steam_command_for(
        &self,
        id: Option<GameId>,
        request: SteamRequest,
    ) -> Result<crate::host::CommandSpec> {
        let scan = self
            .lib
            .steam_scan()
            .ok_or_else(|| Error::new(ErrorCode::StmNotFound))?;
        let (kind, root) = match id.and_then(|i| i.steam_app_id()) {
            Some(app) => {
                let g = scan
                    .game(app)
                    .ok_or_else(|| crate::library::steam::not_listed(app))?;
                (g.facts.install, g.root.clone())
            }
            None => {
                let r = scan
                    .roots
                    .first()
                    .ok_or_else(|| Error::new(ErrorCode::StmNotFound))?;
                (r.kind, r.path.clone())
            }
        };
        command(
            &*self.deps.fs,
            kind,
            &root,
            &self.deps.launch_env.host_env,
            request,
        )
        .ok_or_else(|| Error::new(ErrorCode::StmClientUnavailable).with("install", kind.as_str()))
    }

    /// Hand `request` to Steam on the job pool.
    pub(crate) fn steam_ask(&mut self, game: Option<GameId>, request: SteamRequest) {
        if let Some(id) = game
            && let Err(e) = self.steam_summary(id)
        {
            return self.fail(e);
        }
        let spec = match self.steam_command_for(game, request) {
            Ok(s) => s,
            Err(e) => return self.fail(e),
        };
        let Some(handoff) = self.flow.steam.handoff.clone() else {
            return self
                .fail(Error::new(ErrorCode::StmClientUnavailable).with("detail", "no hand-off"));
        };
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Interactive, JobLabel("steam-ask"), move |_| {
                let result = handoff.start_detached(&spec);
                post(Internal::Flow(Box::new(FlowDone::Steam(
                    SteamDone::Asked {
                        game,
                        request,
                        result,
                    },
                ))));
            });
    }

    fn steam_details(&mut self, id: GameId, reply_to: QueryId) {
        let result = match self.steam_detail(id) {
            Ok(d) => QueryResult::Steam(Box::new(d)),
            Err(e) => QueryResult::Failed(Box::new(self.describe(&e, Some(id)))),
        };
        self.emit(Event::QueryResult {
            id: reply_to,
            result,
        });
    }

    fn steam_detail(&mut self, id: GameId) -> Result<SteamDetail> {
        let s = self.steam_summary(id)?;
        let app = id.steam_app_id().unwrap_or(0);
        let crate::model::GameOrigin::Steam(facts) = s.origin.clone() else {
            return Err(crate::library::steam::not_listed(app));
        };
        let notes = self.lib.steam_row(app)?.notes;
        let running = self.flow.steam.is_running(app);
        let client_found = self
            .steam_command_for(Some(id), SteamRequest::Run(app))
            .is_ok();
        let prefix_tools = !running
            && matches!(
                &facts.compat,
                SteamCompat::Proton {
                    prefix: Some(_),
                    proton_dir: Some(_),
                    ..
                }
            );
        Ok(SteamDetail {
            game: id,
            facts,
            notes,
            running,
            client_found,
            prefix_tools,
        })
    }

    fn steam_open_folder(&mut self, id: GameId, prefix: bool) {
        let s = match self.steam_summary(id) {
            Ok(s) => s,
            Err(e) => return self.fail(e),
        };
        let Some(f) = s.steam() else { return };
        let path: PathBuf = if prefix {
            match f.prefix() {
                Some(p) => p.to_path_buf(),
                None => return self.fail(Error::new(ErrorCode::StmNoPrefix)),
            }
        } else {
            f.install_dir.clone()
        };
        let Some(d) = self.deps.desktop.clone() else {
            return self.fail(Error::not_available("DESKTOP-005"));
        };
        let fs = Arc::clone(&self.deps.fs);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("desktop-open"),
            move |_| {
                let done = if !fs.metadata_follow(&path).is_ok_and(|m| m.is_dir()) {
                    DesktopDone::Failed(
                        Error::new(ErrorCode::FsReadDenied).with("path", path.display()),
                    )
                } else {
                    match d.opener.open(&KnownPath::folder(path)) {
                        Ok(()) => DesktopDone::Done,
                        Err(e) => DesktopDone::Failed(e),
                    }
                };
                post(Internal::Desktop(Box::new(done)));
            },
        );
    }

    /// Run `tool` in the Steam game's Windows storage (ADR-0031 §6).
    fn steam_run_in_prefix(&mut self, id: GameId, tool: Tool) {
        if let Err(e) = tool.validate() {
            return self.fail(e.into());
        }
        if matches!(tool, Tool::Winetricks { .. }) {
            // Needs the network inside an Offline sandbox; not offered for Steam games yet.
            return self.fail(not_for_steam("winetricks"));
        }
        let s = match self.steam_summary(id) {
            Ok(s) => s,
            Err(e) => return self.fail(e),
        };
        let Some(app) = id.steam_app_id() else { return };
        if self.flow.steam.is_running(app) {
            return self.fail(Error::new(ErrorCode::StmGameRunning));
        }
        let Some(facts) = s.steam() else { return };
        let (compat_data, proton) = match &facts.compat {
            SteamCompat::Proton {
                compat_data,
                prefix: Some(_),
                proton_dir,
                ..
            } => match proton_dir {
                Some(d) => (compat_data.clone(), d.clone()),
                None => return self.fail(Error::new(ErrorCode::StmLayerUnavailable)),
            },
            _ => return self.fail(Error::new(ErrorCode::StmNoPrefix)),
        };
        let (program, root) = match &tool {
            // A program elsewhere (a mod installer in Downloads): its folder, read-only. A
            // program inside the prefix is already reachable through it, like winecfg below.
            Tool::RunExe { path, .. } if !path.starts_with(&compat_data) => {
                let root = path
                    .parent()
                    .filter(|d| !self.deps.shared.is_shared_location(d))
                    .map(Path::to_path_buf);
                (path.clone(), root)
            }
            // Windows' own programs come from the prefix: the game's install folder is the
            // (read-only) game folder, so nothing is bound inside the prefix twice.
            _ => (
                facts.install_dir.join("winecfg.exe"),
                Some(facts.install_dir.clone()),
            ),
        };
        let name = s.name.to_string();
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        let paths = self.deps.paths.clone();
        let env = self.deps.launch_env.clone();
        let install_id = self.install_id.clone();
        let runtimes = Arc::clone(&self.deps.runtimes);
        let launcher = Arc::clone(&self.deps.launcher);
        let locks = Arc::clone(&self.deps.locks);
        let host = Arc::clone(&self.deps.host);
        let proc = Arc::clone(&self.deps.proc);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("steam-prefix-tool"),
            move |_| {
                let result = (|| -> Result<SessionId> {
                    // Authoritative check: Steam may have started the game meanwhile.
                    if running_games(&*proc).contains_key(&app) {
                        return Err(Error::new(ErrorCode::StmGameRunning));
                    }
                    if locks.is_held(&paths.prefix_lock(id)) {
                        return Err(Error::new(ErrorCode::SesAlreadyRunning).with("game", id));
                    }
                    // EasyPlay's own folder for this game: the private home and the lock.
                    fs.create_dir_all(&paths.home_dir(id), 0o700)?;
                    let game =
                        tool_game(id, &name, &program, root.as_deref(), &proton, clock.now());
                    let runtime = match runtimes.resolve(&game.runtime, Some(&game)) {
                        Resolution::Ready(rt) => rt,
                        Resolution::NeedsProvision(need) => {
                            return Err(super::not_installed(&need));
                        }
                        Resolution::Unavailable(r) => {
                            return Err(Error::new(ErrorCode::StmLayerUnavailable)
                                .with("reason", format!("{r:?}")));
                        }
                    };
                    let facts = host.facts();
                    let other_roots: Vec<PathBuf> = Vec::new();
                    let henv = env.host_env(&paths, &install_id, &other_roots, &*fs);
                    let req = PlanRequest {
                        session_id: SessionId::generate(&*clock),
                        target: LaunchTarget::Tool(&tool),
                        game: Some(&game),
                        runtime: Some(&runtime),
                        proton_codename: None,
                        network_override: None,
                        host: &facts,
                        env: &henv,
                    };
                    let mut planned = plan_launch_in_prefix(&req, &compat_data)?;
                    steam_tool_plan(&mut planned.plan);
                    Ok(launcher.start(&planned.plan)?.session_id)
                })();
                post(Internal::Flow(Box::new(FlowDone::Tool {
                    game: id,
                    result,
                })));
            },
        );
    }
}
