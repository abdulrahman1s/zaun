//! Game actions shared by cards, rows, the context menu, the selection bar, the game page and
//! shortcuts (GAME_LIBRARY §9–10). Each turns intent into commands, dialogs or navigation.

use easyplay_core::ids::{GameId, TagId};
use easyplay_core::library::RemoveOptions;
use easyplay_core::model::{Activity, BasePreset, GameSummary, NetworkAccess};

use super::Env;
use crate::app::UiState;
use crate::bridge::{Command, FolderRequest, SteamAction, UiCommand};
use crate::input::picker::PickPurpose;
use crate::route::{Overlay, Route};
use crate::strings;
use crate::vm::details::DetailsTab;
use crate::vm::dialogs::{AfterDiscard, Confirm, Dialog};
use crate::vm::screens::ArtworkVm;
use crate::vm::toasts::ToastKind;
use crate::widgets::button::PlayState;
use crate::widgets::card::{PlayView, play_view};

/// A per-game action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameAction {
    /// Play, stop, or locate (by state).
    Play,
    /// Stop.
    Stop,
    /// Open the game page.
    Open,
    /// Toggle favourite.
    Favorite,
    /// Toggle one tag.
    ToggleTag(TagId),
    /// Open the tag editor.
    EditTags,
    /// The `T` key: the tag menu hanging from the focused cover in the library (the tag
    /// editor elsewhere).
    TagMenu,
    /// Switch preset (asks when widening).
    Preset(BasePreset),
    /// Open the Protection tab.
    EditProtection,
    /// Open the game folder.
    OpenFolder,
    /// Open the save data.
    OpenSaveData,
    /// Add or remove the applications-menu shortcut.
    Shortcut(bool),
    /// Run another program in this game's storage.
    RunOther,
    /// Open the artwork sheet.
    Artwork,
    /// Hide or unhide.
    Hide,
    /// Remove (dialog).
    Remove,
    /// The `Delete` key: remove the selection when there is one, else this game.
    Delete,
    /// Copy the launch command (Developer Mode).
    CopyCommand,
    /// Open the launch inspector for the next launch (Developer Mode).
    Inspect,
    /// A Steam game: show it in Steam, whose properties hold its compatibility layer and
    /// launch options (ADR-0031).
    OpenInSteam,
    /// A Steam game: open Steam's downloads, where its update is started.
    UpdateInSteam,
    /// A Steam game run through Proton: open its Windows storage in the file manager.
    OpenStorage,
}

impl GameAction {
    /// Whether the action applies to a Steam game (ADR-0031): Steam runs it, so EasyPlay
    /// cannot stop it, protect it, remove it, give it artwork, a shortcut or another
    /// program, nor inspect its launch.
    pub fn applies_to_steam(self) -> bool {
        !matches!(
            self,
            GameAction::Stop
                | GameAction::Preset(_)
                | GameAction::EditProtection
                | GameAction::OpenSaveData
                | GameAction::Shortcut(_)
                | GameAction::Artwork
                | GameAction::CopyCommand
                | GameAction::Inspect
                | GameAction::Remove
        )
    }
}

/// Whether switching `g` to `preset` widens its access (network is the preset's main
/// difference; Standard widens Online further).
pub fn widens(g: &GameSummary, preset: BasePreset) -> bool {
    let rank = |b: BasePreset| match b {
        BasePreset::Offline => 0,
        BasePreset::Online => 1,
        BasePreset::Standard => 2,
    };
    rank(preset) > rank(g.preset.base())
        || (g.network == NetworkAccess::None && preset != BasePreset::Offline)
}

/// Apply `action` to `id`.
pub fn apply(state: &mut UiState, env: &mut Env<'_>, id: GameId, action: GameAction) {
    let lib = env.bridge.library().clone();
    let Some(g) = lib.get(id) else {
        return;
    };
    if g.is_steam() && !action.applies_to_steam() {
        // "Remove" is the one a person may ask for by key or menu: say how it is done.
        if action == GameAction::Remove {
            state.toast(
                strings::steam::selection::NOT_REMOVED(),
                ToastKind::Info,
                None,
                env.mono,
            );
        }
        return;
    }
    match action {
        GameAction::Play => play(state, env, g),
        GameAction::Stop => {
            if g.activity != Activity::Idle {
                env.bridge.send(UiCommand::Stop { id, force: false });
            }
        }
        GameAction::Open => {
            state.library.selected = Some(id);
            state.navigate(AfterDiscard::Route(Route::Game(id)));
        }
        GameAction::Favorite => {
            let on = !g.favorite;
            env.bridge.send(Command::SetFavorite { id, favorite: on });
            state.toast(
                strings::feedback::favorited(&g.name, on),
                ToastKind::Success,
                Some(UiCommand::Core(Command::SetFavorite { id, favorite: !on })),
                env.mono,
            );
        }
        GameAction::ToggleTag(t) => {
            // Ticked and unticked in a menu that stays open: no toast.
            let mut tags: Vec<TagId> = g.tags.to_vec();
            if let Some(i) = tags.iter().position(|x| *x == t) {
                tags.remove(i);
            } else {
                tags.push(t);
            }
            let names = tags
                .iter()
                .filter_map(|t| lib.tag(*t))
                .filter_map(|t| easyplay_core::ids::TagName::new(&*t.name).ok())
                .collect();
            env.bridge.send(Command::SetTags { id, tags: names });
        }
        GameAction::TagMenu
            if state.nav.current() == Route::Library && state.library.cursor_rect.is_some() =>
        {
            state.library.tag_menu = Some(id);
        }
        GameAction::EditTags | GameAction::TagMenu => {
            state.tag_text.clear();
            state.nav.open(Overlay::Tags(id));
        }
        GameAction::Preset(p) => {
            if widens(g, p) {
                state.show_dialog(Dialog::Confirm(Confirm::WidenProtection {
                    ids: vec![id],
                    preset: p,
                }));
            } else {
                env.bridge.send(UiCommand::ApplyPreset {
                    ids: vec![id],
                    preset: p,
                });
            }
        }
        GameAction::EditProtection => {
            state.navigate(AfterDiscard::Route(Route::Game(id)));
            state.details.tab = DetailsTab::Protection;
        }
        GameAction::OpenFolder => env
            .bridge
            .send(Command::OpenFolder(FolderRequest::GameFolder(id))),
        GameAction::OpenSaveData => env
            .bridge
            .send(Command::OpenFolder(FolderRequest::SaveData(id))),
        GameAction::Shortcut(on) => {
            env.bridge.send(Command::SetShortcut { id, enable: on });
            state.toast(
                strings::feedback::shortcut(&g.name, on),
                ToastKind::Success,
                Some(UiCommand::Core(Command::SetShortcut { id, enable: !on })),
                env.mono,
            );
        }
        // For a Steam game the picked program runs in its Windows storage (`app.rs`).
        GameAction::RunOther => state.picks.push(PickPurpose::RunProgram(id)),
        GameAction::Artwork => {
            state.artwork = Some(ArtworkVm::new(id, None));
            state.nav.open(Overlay::Artwork(id));
        }
        GameAction::Hide => {
            let on = !g.hidden;
            env.bridge.send(Command::SetHidden { id, hidden: on });
            state.library.selection.clear();
            // Hiding offers Undo; showing a game again says nothing.
            if on {
                state.toast(
                    strings::feedback::hidden(&g.name, on),
                    ToastKind::Success,
                    Some(UiCommand::Core(Command::SetHidden { id, hidden: !on })),
                    env.mono,
                );
            }
        }
        GameAction::Remove => {
            state.show_dialog(Dialog::Confirm(Confirm::RemoveGames {
                ids: vec![id],
                options: RemoveOptions::default(),
            }));
        }
        GameAction::Delete => {
            if state.library.selection.is_empty() {
                if g.is_steam() {
                    state.toast(
                        strings::steam::selection::NOT_REMOVED(),
                        ToastKind::Info,
                        None,
                        env.mono,
                    );
                } else {
                    state.show_dialog(Dialog::Confirm(Confirm::RemoveGames {
                        ids: vec![id],
                        options: RemoveOptions::default(),
                    }));
                }
            } else {
                bulk(state, env, BulkAction::Remove);
            }
        }
        GameAction::OpenInSteam => env
            .bridge
            .send(UiCommand::Steam(SteamAction::OpenInSteam(id))),
        GameAction::UpdateInSteam => env.bridge.send(UiCommand::Steam(SteamAction::Update(id))),
        GameAction::OpenStorage => env
            .bridge
            .send(UiCommand::Steam(SteamAction::OpenStorageFolder(id))),
        GameAction::Inspect => super::debug::inspect_next(state, env, id),
        GameAction::CopyCommand => {
            let cmd = strings::menu::launch_command(&g.id.to_string());
            env.ctx.copy_text(cmd);
            state.toast(
                strings::feedback::COPIED(),
                ToastKind::Success,
                None,
                env.mono,
            );
        }
    }
}

/// The Play pill: play, stop, or locate by state. A Steam game is handed to Steam while it
/// is idle and nothing else: never a Stop, never "Locate…" (ADR-0031).
pub fn play(state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let id = g.id;
    state.library.selected = Some(id);
    let st = match play_view(g) {
        PlayView::State(st) => st,
        PlayView::Blocked(_) => return,
    };
    if g.is_steam() {
        if st == PlayState::Ready {
            env.bridge.send(UiCommand::Launch { id, exe: None });
        }
        return;
    }
    match st {
        PlayState::Running => env.bridge.send(UiCommand::Stop { id, force: false }),
        PlayState::Stopping => env.bridge.send(UiCommand::Stop { id, force: true }),
        PlayState::Launching(_) => {}
        PlayState::Unavailable => state.picks.push(PickPurpose::Locate(id)),
        PlayState::Ready | PlayState::NeedsRuntime | PlayState::Installer => {
            if !show_problem(state, env, id) {
                env.bridge.send(UiCommand::Launch { id, exe: None });
            }
        }
    }
}

/// A game with a problem open shows the problem instead of failing to start again: the
/// failed start as its dialog (which offers *Try again*), a session that ended at once on
/// the game's page, where the note about it is. Returns whether there was one to show.
fn show_problem(state: &mut UiState, env: &mut Env<'_>, id: GameId) -> bool {
    if let Some(e) = state.launches.failures.get(&id).cloned() {
        state.show_dialog(Dialog::Error(e));
        return true;
    }
    if state.launches.ended_early.contains_key(&id) {
        env.ctx
            .data_mut(|d| d.insert_temp(egui::Id::unique(("details.show_problem", id)), true));
        state.navigate(AfterDiscard::Route(Route::Game(id)));
        return true;
    }
    false
}

/// Actions on the selection (the filter line while games are selected).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BulkAction {
    /// Add a tag to all.
    AddTag(TagId),
    /// Remove a tag from all.
    RemoveTag(TagId),
    /// Tick or untick a tag: on for all unless all have it (the design's tag menu).
    ToggleTag(TagId),
    /// Favourite all, or unfavourite all when all are favourites.
    Favorite,
    /// Hide all.
    Hide,
    /// Switch all to a level (asks first when that widens any).
    Preset(BasePreset),
    /// Remove all (one dialog).
    Remove,
}

/// Apply an action to the selection.
pub fn bulk(state: &mut UiState, env: &mut Env<'_>, action: BulkAction) {
    let lib = env.bridge.library().clone();
    let mut ids: Vec<GameId> = state.library.selection.clone();
    let mut games: Vec<&GameSummary> = ids.iter().filter_map(|i| lib.get(*i)).collect();
    // A change of protection and a removal skip Steam games: Steam runs them, and they are
    // removed by uninstalling them in Steam (ADR-0031). A toast says how many were left, last
    // (a new toast replaces the one showing).
    let mut skipped = 0;
    if matches!(action, BulkAction::Preset(_) | BulkAction::Remove) {
        skipped = games.iter().filter(|g| g.is_steam()).count();
        games.retain(|g| !g.is_steam());
        ids.retain(|i| !i.is_steam());
    }
    if !games.is_empty() {
        bulk_on(state, env, action, ids, &games, &lib);
    }
    if skipped > 0 {
        let text = if games.is_empty() && action == BulkAction::Remove {
            strings::steam::selection::NOT_REMOVED().to_owned()
        } else {
            strings::steam::selection::skipped(skipped)
        };
        state.toast(text, ToastKind::Info, None, env.mono);
    }
}

/// [`bulk`] on `games` (`ids`), which it applies to.
fn bulk_on(
    state: &mut UiState,
    env: &mut Env<'_>,
    action: BulkAction,
    ids: Vec<GameId>,
    games: &[&GameSummary],
    lib: &crate::bridge::LibraryView,
) {
    let names = |tags: &[TagId]| {
        tags.iter()
            .filter_map(|t| lib.tag(*t))
            .filter_map(|t| easyplay_core::ids::TagName::new(&*t.name).ok())
            .collect::<Vec<_>>()
    };
    match action {
        BulkAction::AddTag(t) | BulkAction::RemoveTag(t) | BulkAction::ToggleTag(t) => {
            let add = match action {
                BulkAction::AddTag(_) => true,
                BulkAction::RemoveTag(_) => false,
                _ => !games.iter().all(|g| g.tags.contains(&t)),
            };
            for g in games {
                let mut tags: Vec<TagId> = g.tags.to_vec();
                tags.retain(|x| *x != t);
                if add {
                    tags.push(t);
                }
                env.bridge.send(Command::SetTags {
                    id: g.id,
                    tags: names(&tags),
                });
            }
        }
        BulkAction::Favorite => {
            let on = !games.iter().all(|g| g.favorite);
            for g in games {
                env.bridge.send(Command::SetFavorite {
                    id: g.id,
                    favorite: on,
                });
            }
            state.toast(
                strings::feedback::favorited_many(games.len(), on),
                ToastKind::Success,
                None,
                env.mono,
            );
        }
        BulkAction::Hide => {
            for g in games {
                env.bridge.send(Command::SetHidden {
                    id: g.id,
                    hidden: true,
                });
            }
            let undo: Vec<UiCommand> = games
                .iter()
                .map(|g| {
                    UiCommand::Core(Command::SetHidden {
                        id: g.id,
                        hidden: false,
                    })
                })
                .collect();
            let text = match games {
                [g] => strings::feedback::hidden(&g.name, true),
                _ => strings::feedback::hid_many(games.len()),
            };
            state.toast_undoing(text, ToastKind::Success, undo, env.mono);
            state.library.selection.clear();
        }
        BulkAction::Preset(p) => {
            if games.iter().any(|g| widens(g, p)) {
                state.show_dialog(Dialog::Confirm(Confirm::WidenProtection { ids, preset: p }));
            } else {
                env.bridge.send(UiCommand::ApplyPreset { ids, preset: p });
                state.toast(
                    strings::feedback::set_to(games.len(), strings::shield::preset_name(p.into())),
                    ToastKind::Success,
                    None,
                    env.mono,
                );
            }
        }
        BulkAction::Remove => {
            state.show_dialog(Dialog::Confirm(Confirm::RemoveGames {
                ids,
                options: RemoveOptions::default(),
            }));
        }
    }
}

/// Carry out a recovery action offered by a problem panel or an error dialog.
pub fn recover(
    state: &mut UiState,
    env: &mut Env<'_>,
    err: &easyplay_core::error::UserError,
    action: easyplay_core::error::RecoveryAction,
) {
    use easyplay_core::error::RecoveryAction as R;
    let game = err.game;
    if let Some(g) = game {
        state.launches.tried(g, action);
    }
    let page = |state: &mut UiState, tab: DetailsTab| {
        if let Some(g) = game {
            state.nav.push(Route::Game(g));
            state.details.tab = tab;
        }
    };
    match action {
        R::Retry => {
            if let Some(g) = game {
                env.bridge.send(UiCommand::Launch { id: g, exe: None });
            }
        }
        R::EditPermissions | R::ChooseGameFolder | R::AllowGameFolderWrites => {
            page(state, DetailsTab::Protection);
        }
        R::LocateExecutable => {
            if let Some(g) = game {
                state.picks.push(PickPurpose::Locate(g));
            }
        }
        R::RemoveGame => {
            if let Some(g) = game {
                state.show_dialog(Dialog::Confirm(Confirm::RemoveGames {
                    ids: vec![g],
                    options: RemoveOptions::default(),
                }));
            }
        }
        R::SwitchPreset(p) => {
            if let Some(g) = game {
                env.bridge.send(UiCommand::ApplyPreset {
                    ids: vec![g],
                    preset: p,
                });
            }
        }
        R::ChangeRuntime => page(state, DetailsTab::Compatibility),
        R::UseDefaultRuntime => {
            if let Some(g) = game {
                env.bridge.send(UiCommand::UpdateGame {
                    id: g,
                    edits: vec![crate::bridge::GameEdit::Runtime {
                        reference: easyplay_core::model::RuntimeRef::Default,
                        follow: true,
                    }],
                });
            }
        }
        R::DownloadRuntime => env.bridge.send(Command::ProvisionRuntime {
            target: easyplay_core::model::RuntimeRef::Default,
        }),
        R::ResetPrefix => {
            if let Some(g) = game {
                state.show_dialog(Dialog::Confirm(Confirm::ResetPrefix {
                    id: g,
                    options: easyplay_core::library::ResetOptions::default(),
                }));
            }
        }
        R::OpenDoctor => {
            state.onboarding.reduced = true;
            state.onboarding.step = crate::vm::screens::OnboardingStep::SystemCheck;
            state.nav.go_to(Route::Onboarding);
        }
        R::OpenSettings(t) => {
            use crate::route::SettingsTab as S;
            use easyplay_core::error::SettingsTab as C;
            let tab = match t {
                C::General => S::General,
                C::Appearance => S::Appearance,
                C::Protection => S::Protection,
                C::Runtimes => S::CompatibilityLayers,
                C::Integration => S::Integration,
                C::Advanced => S::Advanced,
                C::About => S::About,
            };
            state.nav.go_to(Route::Settings(tab));
        }
        R::ViewLog => match game {
            // The in-app viewer (LOGGING §3.3), also without Developer Mode.
            Some(g) => super::debug::open_logs(state, env, g, err.session),
            None => env.bridge.send(Command::ViewLog {
                session: err.session,
                game,
            }),
        },
        R::OpenSessionFolder => {
            if let Some(s) = err.session {
                env.bridge
                    .send(Command::OpenFolder(FolderRequest::Session(s)));
            }
        }
        R::CopyDetails => {
            env.ctx.copy_text(strings::errors::copy_text(err));
            state.toast(
                strings::feedback::COPIED(),
                ToastKind::Success,
                None,
                env.mono,
            );
        }
        R::CreateReport => super::debug::open_report(state, env, game, err.session),
        R::OpenBackupsFolder => env.bridge.send(Command::OpenFolder(FolderRequest::Backups)),
        // The recovery state offers the backups; otherwise Settings → Advanced lists them.
        R::RestoreBackup => {
            if env.bridge.diagnostics().recovery.is_none() {
                state
                    .nav
                    .go_to(Route::Settings(crate::route::SettingsTab::Advanced));
            }
        }
        // Settings → Advanced lists what EasyPlay stores, with sizes (ERROR_HANDLING §4.14).
        R::FreeSpaceInfo => state
            .nav
            .go_to(Route::Settings(crate::route::SettingsTab::Advanced)),
        R::ShowRunningGame => {
            if let Some(g) = game {
                state.nav.push(Route::Game(g));
            }
        }
        R::Quit => env.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        R::Dismiss => {
            if let Some(g) = game {
                state.launches.dismiss(g);
            }
        }
    }
}
