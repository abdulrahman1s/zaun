//! Which commands go to Steam, and what each Steam command does (ADR-0031).
use super::details::app_id;
use crate::library::steam::not_for_steam;
use crate::service::command::FolderRequest;
use crate::service::prelude::*;
use crate::steam::client::SteamRequest;

impl AppService {
    /// Handle `cmd` when it is a Steam command or addressed to a Steam game; otherwise give
    /// it back. `RescanMissing` also reads Steam's files again and is given back.
    pub(crate) fn steam_route(&mut self, cmd: Command) -> Option<Command> {
        match self.route_steam(cmd) {
            Ok(rest) => rest,
            Err(e) => {
                self.fail(e);
                None
            }
        }
    }

    fn route_steam(&mut self, cmd: Command) -> Result<Option<Command>> {
        let steam = |id: &GameId| id.steam_app_id().is_some();
        match cmd {
            Command::Steam(c) => self.steam_command(c).map(|()| None),
            Command::RescanMissing => {
                self.steam_scan(false);
                Ok(Some(Command::RescanMissing))
            }
            Command::SetFavorite { id, favorite } if steam(&id) => {
                self.steam_set_favorite(id, favorite).map(|()| None)
            }
            Command::SetHidden { id, hidden } if steam(&id) => {
                self.steam_set_hidden(id, hidden).map(|()| None)
            }
            Command::SetTags { id, tags } if steam(&id) => {
                self.steam_set_tags(id, &tags).map(|()| None)
            }
            Command::UpdateGame { id, edits } if steam(&id) => {
                self.steam_update_game(id, edits).map(|()| None)
            }
            Command::Launch { id, .. } if steam(&id) => self
                .steam_ask(Some(id), SteamRequest::Run(app_id(id)?))
                .map(|()| None),
            Command::LoadGame { id, reply_to } if steam(&id) => {
                self.steam_details(id, reply_to).map(|()| None)
            }
            // Nothing Zaun started can be cancelled: Steam owns the launch.
            Command::CancelLaunch { id } if steam(&id) => Ok(None),
            Command::RunTool { id, tool } if steam(&id) => {
                self.steam_run_in_prefix(id, tool).map(|()| None)
            }
            Command::OpenFolder(FolderRequest::GameFolder(id)) if steam(&id) => {
                self.steam_open_folder(id, false).map(|()| None)
            }
            Command::OpenFolder(FolderRequest::Prefix(id)) if steam(&id) => {
                self.steam_open_folder(id, true).map(|()| None)
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
                if steam(&id) =>
            {
                Err(not_for_steam("game action"))
            }
            other => Ok(Some(other)),
        }
    }

    fn steam_command(&mut self, c: SteamCommand) -> Result<()> {
        match c {
            SteamCommand::SetEnabled(on) => {
                let patch = crate::config::PreferencePatch::new().set("steam.show_games", on);
                self.handle(Command::SetPreference(patch));
                Ok(())
            }
            SteamCommand::Rescan => {
                if !self.flow.steam.enabled {
                    return Err(not_for_steam("scan while the option is off"));
                }
                self.steam_scan(true);
                Ok(())
            }
            SteamCommand::Update(id) => self.steam_ask(Some(id), SteamRequest::Downloads),
            SteamCommand::OpenInSteam(id) => {
                self.steam_ask(Some(id), SteamRequest::Details(app_id(id)?))
            }
            SteamCommand::SetLaunchOptions {
                id,
                options,
                reply_to,
            } => self.steam_set_launch_options(id, options, reply_to),
            SteamCommand::CloseSteam(id) => self.steam_close(id),
            SteamCommand::OpenPrefixFolder(id) => self.steam_open_folder(id, true),
            SteamCommand::RunInPrefix { id, tool } => self.steam_run_in_prefix(id, tool),
            SteamCommand::Details { id, reply_to } => self.steam_details(id, reply_to),
        }
    }
}
