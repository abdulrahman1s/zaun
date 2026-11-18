//! The routes table: commands, job completions and prompt answers to their flows (ADR-0034).
use super::app_service::{AppService, Internal};
use super::command::Command;
use super::event::Event;
use super::flows::FlowDone;
use super::prompts::Continuation;
use crate::error::Result;
use crate::ids::{LaunchId, PromptId};
use crate::model::PromptAnswer;

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
            Command::OpenPaths { paths, origin } => self.open_paths(paths, origin),
            Command::CreateGame { draft, then } => self.create_game(*draft, then),
            Command::DiscardDraft => self.setup_closed(),
            Command::RemoveGame { id, options } => self.remove_game(id, options),
            Command::SetFavorite { id, favorite } => self.set_favorite(id, favorite),
            Command::SetHidden { id, hidden } => self.set_hidden(id, hidden),
            Command::SetTags { id, tags } => self.set_tags(id, &tags),
            Command::UpdateGame { id, edits } => self.update_game(id, edits),
            Command::LoadGame { id, reply_to } => self.load_game(id, reply_to),
            Command::AboutGame { id, reply_to } => self.about_game(id, reply_to),
            Command::QueryStats { request, reply_to } => self.query_stats(&request, reply_to),
            Command::RescanMissing => {
                self.check_availability();
                Ok(())
            }
            Command::ClearStatistics => self.clear_statistics(None),
            Command::ResetGameStatistics { id } => self.clear_statistics(Some(id)),
            Command::RemoveSession { id } => self.remove_session(id),
            Command::AddExecutable { id, path, role } => self.add_program(id, &path, role),
            Command::LocateExecutable { id, exe, new_path } => {
                self.locate_program(id, exe, &new_path)
            }
            Command::ScanFolder { folder, reply_to } => self.scan_folder(folder, reply_to),
            Command::MeasurePrefix { id, reply_to } => self.measure_prefix(id, reply_to),
            Command::CancelLaunch { id } => {
                self.cancel_launch(id);
                Ok(())
            }
            Command::RunTool { id, tool } => self.run_tool(id, tool, None),
            Command::ImportArtwork { id, slot, source } => self.import_artwork(id, slot, source),
            Command::FetchOnlineArtwork { id } => self.fetch_online_artwork(id, true),
            Command::FindSteamApp { id, term, reply_to } => self.find_steam_app(id, term, reply_to),
            Command::Launch { id, exe, origin } => {
                self.start_launch(id, exe, origin);
                Ok(())
            }
            Command::StopSession { session, mode } => {
                self.stop_session(session, mode);
                Ok(())
            }
            Command::AnswerPrompt { prompt, answer } => self.answer_prompt(prompt, answer),
            Command::ResetPrefix { id, options } => self.reset_prefix(id, options),
            Command::AdoptRuntime { variant } => self.adopt_runtime(variant),
            Command::RefreshRuntimes => {
                self.scan_runtimes();
                Ok(())
            }
            Command::ProvisionRuntime { target } => self.provision_runtime(target),
            Command::RemoveRuntime { name } => self.remove_runtime(name),
            Command::VerifyRuntime => self.verify_runtimes(),
            Command::SetPreference(patch) => self.set_preference(&patch),
            Command::RefreshHostFacts => {
                self.probe_host(true);
                Ok(())
            }
            Command::SetDefaultHandler { enable } => self.set_default_handler(enable),
            Command::SetShortcut { id, enable } => self.set_shortcut(id, enable),
            Command::OpenFolder(target) => self.open_folder(target),
            Command::ViewLog { session, game } => self.view_log(session, game),
            Command::Inspect { target, reply_to } => self.inspect(target, reply_to),
            Command::ListSessions { game, reply_to } => self.list_sessions(game, reply_to),
            Command::ReadLog {
                session,
                file,
                offset,
                reply_to,
            } => self.read_log(session, file, offset, reply_to),
            Command::ListProcesses { session, reply_to } => self.list_processes(session, reply_to),
            Command::RunDoctor => {
                self.run_doctor();
                Ok(())
            }
            Command::BuildReport {
                game,
                session,
                reply_to,
            } => self.build_report(game, session, reply_to),
            Command::SaveReport { files, dest } => self.save_report(files, dest),
            Command::DismissCrash => self.dismiss_crash(),
            Command::ResolveDatabase(choice) => self.resolve_database(&choice),
            // Handled by `steam_route`.
            Command::Steam(_) => Ok(()),
            Command::Chat(c) => self.chat_command(c),
            Command::Overlay(a) => {
                self.overlay_action(a);
                Ok(())
            }
            Command::Updates(c) => self.update_command(c),
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
                // The runtime check (D-15) reads the catalog.
                if self.hub.diagnostics.load().doctor.is_some() {
                    self.run_doctor();
                }
                Ok(())
            }
            Internal::Sessions(p) => {
                self.on_sessions(p);
                Ok(())
            }
            Internal::Art(a) => self.on_art_fetched(*a),
            Internal::Adopted(r) => self.on_adopted(r),
            Internal::RuntimeJob(d) => self.on_runtime_job(d),
            Internal::Flow(f) => self.on_flow_done(*f),
            Internal::Desktop(d) => self.on_desktop(*d),
            Internal::Doctor(r) => self.on_doctor(*r),
            Internal::ReportSaved(r) => self.on_report_saved(r),
        }
    }

    /// A job of the flows finished.
    fn on_flow_done(&mut self, done: FlowDone) -> Result<()> {
        match done {
            FlowDone::Removed { job, files } => self.on_removed(&job, files),
            FlowDone::Reset { game, result } => self.on_reset(game, result),
            FlowDone::Tool { game, result } => self.on_tool_started(game, result),
            FlowDone::Query { id, result } => {
                self.emit(Event::QueryResult { id, result });
                Ok(())
            }
            FlowDone::Maintenance(r) => self.on_maintenance(r),
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
            FlowDone::Steam(d) => self.on_steam_done(d),
            FlowDone::Chat(d) => self.on_chat_done(d),
            FlowDone::ChatServices(d) => self.on_services_done(d),
            FlowDone::Overlay(d) => self.on_overlay_done(d),
            FlowDone::Updates(d) => self.on_updates_done(d),
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
                self.resume_open(c, &answer)
            }
            c @ (Continuation::LaunchExeChanged { .. } | Continuation::LaunchRuntime { .. }) => {
                self.resume_launch(c, &answer)
            }
            c @ Continuation::RemoveRunning { .. } => self.resume_remove(c, &answer),
            c @ Continuation::ToolNetwork { .. } => self.resume_tool(c, &answer),
            c @ Continuation::Locate { .. } => self.resume_locate(c, &answer),
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
                // Updating may be the way out (a library written by a newer Zaun).
                | Command::Updates(_)
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
