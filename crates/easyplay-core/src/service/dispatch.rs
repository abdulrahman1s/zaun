//! The routes table: commands, job completions and prompt answers to their flows (ADR-0034).

#![allow(
    clippy::unit_arg,
    reason = "`Reply` bridges handlers still returning `()`; goes when every handler returns `Result`"
)]

use super::app_service::{AppService, Internal};
use super::command::Command;
use super::event::Event;
use super::flows::FlowDone;
use super::flows::chat_services;
use super::prompts::Continuation;
use crate::error::Result;
use crate::ids::{LaunchId, PromptId};
use crate::model::PromptAnswer;

/// What a handler hands back: `()` or `Result<()>`, like axum's `IntoResponse`. Lets flows move
/// to `Result<()>` one at a time; removed once all of them have (ADR-0034).
pub(crate) trait Reply {
    fn into_result(self) -> Result<()>;
}

impl Reply for () {
    fn into_result(self) -> Result<()> {
        Ok(())
    }
}

impl Reply for Result<()> {
    fn into_result(self) -> Result<()> {
        self
    }
}

impl AppService {
    /// Handle one command (also used by tests and the headless CLI through
    /// [`AppService::open_headless`]).
    pub fn handle(&mut self, cmd: Command) {
        if self.refuse_in_recovery(&cmd) {
            return;
        }
        // Steam commands and commands addressed to a Steam game (ADR-0031).
        let Some(cmd) = self.steam_route(cmd) else {
            return;
        };
        if let Err(e) = self.route(cmd) {
            self.fail(e);
        }
    }

    fn route(&mut self, cmd: Command) -> Result<()> {
        match cmd {
            Command::OpenPaths { paths, origin } => self.open_paths(paths, origin).into_result(),
            Command::CreateGame { draft, then } => self.create_game(*draft, then).into_result(),
            Command::DiscardDraft => self.setup_closed().into_result(),
            Command::RemoveGame { id, options } => self.remove_game(id, options).into_result(),
            Command::SetFavorite { id, favorite } => self.set_favorite(id, favorite),
            Command::SetHidden { id, hidden } => self.set_hidden(id, hidden),
            Command::SetTags { id, tags } => self.set_tags(id, &tags),
            Command::UpdateGame { id, edits } => self.update_game(id, edits),
            Command::LoadGame { id, reply_to } => self.load_game(id, reply_to),
            Command::AboutGame { id, reply_to } => self.about_game(id, reply_to).into_result(),
            Command::QueryStats { request, reply_to } => self.query_stats(&request, reply_to),
            Command::RescanMissing => self.check_availability().into_result(),
            Command::ClearStatistics => self.clear_statistics(None).into_result(),
            Command::ResetGameStatistics { id } => self.clear_statistics(Some(id)).into_result(),
            Command::RemoveSession { id } => self.remove_session(id).into_result(),
            Command::AddExecutable { id, path, role } => {
                self.add_program(id, &path, role).into_result()
            }
            Command::LocateExecutable { id, exe, new_path } => {
                self.locate_program(id, exe, &new_path).into_result()
            }
            Command::ScanFolder { folder, reply_to } => {
                self.scan_folder(folder, reply_to).into_result()
            }
            Command::MeasurePrefix { id, reply_to } => {
                self.measure_prefix(id, reply_to).into_result()
            }
            Command::CancelLaunch { id } => self.cancel_launch(id).into_result(),
            Command::RunTool { id, tool } => self.run_tool(id, tool, None).into_result(),
            Command::ImportArtwork { id, slot, source } => {
                self.import_artwork(id, slot, source).into_result()
            }
            Command::FetchOnlineArtwork { id } => self.fetch_online_artwork(id, true).into_result(),
            Command::FindSteamApp { id, term, reply_to } => {
                self.find_steam_app(id, term, reply_to).into_result()
            }
            Command::Launch { id, exe, origin } => self.start_launch(id, exe, origin).into_result(),
            Command::StopSession { session, mode } => {
                self.stop_session(session, mode).into_result()
            }
            Command::AnswerPrompt { prompt, answer } => self.answer_prompt(prompt, answer),
            Command::ResetPrefix { id, options } => self.reset_prefix(id, options).into_result(),
            Command::AdoptRuntime { variant } => self.adopt_runtime(variant).into_result(),
            Command::RefreshRuntimes => self.scan_runtimes().into_result(),
            Command::ProvisionRuntime { target } => self.provision_runtime(target).into_result(),
            Command::RemoveRuntime { name } => self.remove_runtime(name).into_result(),
            Command::VerifyRuntime => self.verify_runtimes().into_result(),
            Command::SetPreference(patch) => self.set_preference(&patch),
            Command::RefreshHostFacts => {
                self.probe_host(true);
                Ok(())
            }
            Command::SetDefaultHandler { enable } => self.set_default_handler(enable).into_result(),
            Command::SetShortcut { id, enable } => self.set_shortcut(id, enable).into_result(),
            Command::OpenFolder(target) => self.open_folder(target).into_result(),
            Command::ViewLog { session, game } => self.view_log(session, game).into_result(),
            Command::Inspect { target, reply_to } => self.inspect(target, reply_to).into_result(),
            Command::ListSessions { game, reply_to } => {
                self.list_sessions(game, reply_to).into_result()
            }
            Command::ReadLog {
                session,
                file,
                offset,
                reply_to,
            } => self.read_log(session, file, offset, reply_to).into_result(),
            Command::ListProcesses { session, reply_to } => {
                self.list_processes(session, reply_to).into_result()
            }
            Command::RunDoctor => self.run_doctor().into_result(),
            Command::BuildReport {
                game,
                session,
                reply_to,
            } => self.build_report(game, session, reply_to).into_result(),
            Command::SaveReport { files, dest } => self.save_report(files, dest).into_result(),
            Command::DismissCrash => self.dismiss_crash().into_result(),
            Command::ResolveDatabase(choice) => self.resolve_database(&choice),
            // Handled by `steam_route`.
            Command::Steam(_) => Ok(()),
            Command::Chat(c) if chat_services::handles(&c) => {
                self.chat_service_command(c).into_result()
            }
            Command::Chat(c) => self.chat_command(c).into_result(),
            Command::Shutdown => {
                self.stopping = true;
                Ok(())
            }
        }
    }

    pub(super) fn handle_internal(&mut self, i: Internal) {
        if let Err(e) = self.route_internal(i) {
            self.fail(e);
        }
    }

    fn route_internal(&mut self, i: Internal) -> Result<()> {
        match i {
            Internal::Opened(o) => self.on_opened(*o).into_result(),
            Internal::Drafted { result, origin } => self.on_drafted(result, origin).into_result(),
            Internal::LaunchState {
                game,
                launch,
                state,
            } => self.on_launch_state(game, launch, state).into_result(),
            Internal::LaunchDone(d) => self.on_launch_done(*d).into_result(),
            Internal::HostFacts(f) => self.on_host_facts(f),
            Internal::Runtimes(c) => {
                self.hub.runtimes.store(c);
                self.emit(Event::RuntimesChanged);
                // The runtime check (D-15) reads the catalog.
                if self.hub.diagnostics.load().doctor.is_some() {
                    self.run_doctor();
                }
                Ok(())
            }
            Internal::Sessions(p) => self.on_sessions(p).into_result(),
            Internal::Art(a) => self.on_art_fetched(*a).into_result(),
            Internal::Adopted(r) => self.on_adopted(r).into_result(),
            Internal::RuntimeJob(d) => self.on_runtime_job(d).into_result(),
            Internal::Flow(f) => self.on_flow_done(*f),
            Internal::Desktop(d) => self.on_desktop(*d).into_result(),
            Internal::Doctor(r) => self.on_doctor(*r).into_result(),
            Internal::ReportSaved(r) => self.on_report_saved(r).into_result(),
        }
    }

    /// A job of the flows finished.
    fn on_flow_done(&mut self, done: FlowDone) -> Result<()> {
        match done {
            FlowDone::Removed { job, files } => self.on_removed(&job, files).into_result(),
            FlowDone::Reset { game, result } => self.on_reset(game, result).into_result(),
            FlowDone::Tool { game, result } => self.on_tool_started(game, result).into_result(),
            FlowDone::Query { id, result } => {
                self.emit(Event::QueryResult { id, result });
                Ok(())
            }
            FlowDone::Maintenance(r) => self.on_maintenance(r).into_result(),
            FlowDone::Availability(a) => {
                self.lib.set_availability(&a);
                self.publish_library();
                Ok(())
            }
            FlowDone::Accents(a) => {
                self.lib.set_accents(&a);
                self.publish_library();
                Ok(())
            }
            FlowDone::Steam(d) => self.on_steam_done(d).into_result(),
            FlowDone::Chat(d) => self.on_chat_done(d).into_result(),
            FlowDone::ChatServices(d) => self.on_services_done(d).into_result(),
        }
    }

    /// `AnswerPrompt`: resume the parked flow (a stale or foreign answer is ignored).
    fn answer_prompt(&mut self, prompt: PromptId, answer: PromptAnswer) -> Result<()> {
        let Some(parked) = self.prompts.take(prompt, &answer) else {
            tracing::info!(%prompt, "answer for an unknown prompt or not one of its choices");
            return Ok(());
        };
        match parked.then {
            c @ (Continuation::OpenChanged { .. } | Continuation::OpenAttach { .. }) => {
                self.resume_open(c, &answer).into_result()
            }
            c @ (Continuation::LaunchExeChanged { .. } | Continuation::LaunchRuntime { .. }) => {
                self.resume_launch(c, &answer).into_result()
            }
            c @ Continuation::RemoveRunning { .. } => self.resume_remove(c, &answer).into_result(),
            c @ Continuation::ToolNetwork { .. } => self.resume_tool(c, &answer).into_result(),
            c @ Continuation::Locate { .. } => self.resume_locate(c, &answer).into_result(),
            Continuation::Nothing => Ok(()),
        }
    }

    /// In recovery mode, refuse what would write the library or start a session, with the
    /// recovery error (ERROR_HANDLING §5). Returns whether `cmd` was refused.
    fn refuse_in_recovery(&mut self, cmd: &Command) -> bool {
        let Some(r) = self.hub.diagnostics.load().recovery.clone() else {
            return false;
        };
        let allowed = matches!(
            cmd,
            Command::LoadGame { .. }
                | Command::AboutGame { .. }
                | Command::QueryStats { .. }
                | Command::ScanFolder { .. }
                | Command::MeasurePrefix { .. }
                | Command::FindSteamApp { .. }
                | Command::RescanMissing
                | Command::StopSession { .. }
                | Command::AnswerPrompt { .. }
                | Command::RefreshRuntimes
                | Command::SetPreference(_)
                | Command::RefreshHostFacts
                | Command::OpenFolder(_)
                | Command::ViewLog { .. }
                | Command::Inspect { .. }
                | Command::ListSessions { .. }
                | Command::ReadLog { .. }
                | Command::ListProcesses { .. }
                | Command::RunDoctor
                | Command::BuildReport { .. }
                | Command::SaveReport { .. }
                | Command::DismissCrash
                | Command::ResolveDatabase(_)
                | Command::Shutdown
        );
        if allowed {
            return false;
        }
        tracing::info!(command = cmd.name(), "refused in recovery mode");
        let u = r.error.clone();
        if let Command::Launch { id, .. } = cmd {
            let launch = LaunchId::generate(&*self.deps.clock);
            let mut u = u;
            u.game = Some(*id);
            self.emit(Event::LaunchProgress {
                game: *id,
                launch,
                state: crate::launch::LaunchState::Failed(Box::new(u)),
            });
        } else {
            self.emit(Event::Failure(Box::new(u)));
        }
        true
    }
}
