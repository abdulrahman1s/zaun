//! Game actions shared by cards, rows, the context menu, the selection bar, the game page and
//! shortcuts (GAME_LIBRARY §9–10). Each turns intent into commands, dialogs or navigation.

use easyplay_core::ids::{GameId, TagId};
use easyplay_core::library::RemoveOptions;
use easyplay_core::model::{Activity, BasePreset, GameSummary, NetworkAccess};

use super::Env;
use crate::app::UiState;
use crate::bridge::{FolderTarget, UiCommand};
use crate::input::picker::PickPurpose;
use crate::route::{Overlay, Route};
use crate::strings;
use crate::vm::details::DetailsTab;
use crate::vm::dialogs::{AfterDiscard, Confirm, Dialog};
use crate::vm::screens::ArtworkVm;
use crate::vm::toasts::ToastKind;
use crate::widgets::button::PlayState;
use crate::widgets::card::play_state;

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
    /// Copy the launch command (Developer Mode).
    CopyCommand,
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
            env.bridge.send(UiCommand::SetFavorite { id, favorite: on });
            state.toast(
                strings::feedback::favorited(&g.name, on),
                ToastKind::Success,
                Some(UiCommand::SetFavorite { id, favorite: !on }),
                env.mono,
            );
        }
        GameAction::ToggleTag(t) => {
            let mut tags: Vec<TagId> = g.tags.to_vec();
            if let Some(i) = tags.iter().position(|x| *x == t) {
                tags.remove(i);
            } else {
                tags.push(t);
            }
            let names = |ids: &[TagId]| {
                ids.iter()
                    .filter_map(|t| lib.tag(*t))
                    .filter_map(|t| easyplay_core::ids::TagName::new(&*t.name).ok())
                    .collect::<Vec<_>>()
            };
            let before = names(&g.tags);
            env.bridge.send(UiCommand::SetTags {
                id,
                tags: names(&tags),
            });
            state.toast(
                strings::feedback::tags_changed(&g.name),
                ToastKind::Success,
                Some(UiCommand::SetTags { id, tags: before }),
                env.mono,
            );
        }
        GameAction::EditTags => {
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
            .send(UiCommand::OpenFolder(FolderTarget::GameFolder(id))),
        GameAction::OpenSaveData => env
            .bridge
            .send(UiCommand::OpenFolder(FolderTarget::SaveData(id))),
        GameAction::Shortcut(on) => {
            env.bridge.send(UiCommand::SetShortcut { id, enable: on });
            state.toast(
                strings::feedback::shortcut(&g.name, on),
                ToastKind::Success,
                Some(UiCommand::SetShortcut { id, enable: !on }),
                env.mono,
            );
        }
        GameAction::RunOther => state.picks.push(PickPurpose::RunProgram(id)),
        GameAction::Artwork => {
            state.artwork = Some(ArtworkVm::new(id, None));
            state.nav.open(Overlay::Artwork(id));
        }
        GameAction::Hide => {
            let on = !g.hidden;
            env.bridge.send(UiCommand::SetHidden { id, hidden: on });
            state.toast(
                strings::feedback::hidden(&g.name, on),
                ToastKind::Success,
                Some(UiCommand::SetHidden { id, hidden: !on }),
                env.mono,
            );
        }
        GameAction::Remove => {
            state.show_dialog(Dialog::Confirm(Confirm::RemoveGames {
                ids: vec![id],
                options: RemoveOptions::default(),
            }));
        }
        GameAction::CopyCommand => {
            let cmd = format!("easyplay launch {}", g.id);
            env.ctx.copy_text(cmd);
            state.toast(
                strings::feedback::COPIED,
                ToastKind::Success,
                None,
                env.mono,
            );
        }
    }
}

/// The Play pill: play, stop, or locate by state.
pub fn play(state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let id = g.id;
    state.library.selected = Some(id);
    match play_state(g) {
        PlayState::Running => env.bridge.send(UiCommand::Stop { id, force: false }),
        PlayState::Stopping => env.bridge.send(UiCommand::Stop { id, force: true }),
        PlayState::Launching(_) => {}
        PlayState::Unavailable => state.picks.push(PickPurpose::Locate(id)),
        PlayState::Ready | PlayState::NeedsRuntime | PlayState::Installer => {
            env.bridge.send(UiCommand::Launch { id, exe: None });
        }
    }
}

/// Bulk actions of the selection bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BulkAction {
    /// Add a tag to all.
    AddTag(TagId),
    /// Remove a tag from all.
    RemoveTag(TagId),
    /// Favourite all.
    Favorite,
    /// Hide all.
    Hide,
    /// Switch all to a preset (summary dialog when widening).
    Preset(BasePreset),
    /// Remove all (one dialog).
    Remove,
}

/// Apply a bulk action to the selection; reports "3 changed, 1 skipped" for presets.
pub fn bulk(state: &mut UiState, env: &mut Env<'_>, action: BulkAction) {
    let lib = env.bridge.library().clone();
    let ids: Vec<GameId> = state.library.selection.clone();
    let games: Vec<&GameSummary> = ids.iter().filter_map(|i| lib.get(*i)).collect();
    match action {
        BulkAction::AddTag(t) | BulkAction::RemoveTag(t) => {
            let add = matches!(action, BulkAction::AddTag(_));
            for g in &games {
                let mut tags: Vec<TagId> = g.tags.to_vec();
                tags.retain(|x| *x != t);
                if add {
                    tags.push(t);
                }
                let names = tags
                    .iter()
                    .filter_map(|t| lib.tag(*t))
                    .filter_map(|t| easyplay_core::ids::TagName::new(&*t.name).ok())
                    .collect();
                env.bridge.send(UiCommand::SetTags {
                    id: g.id,
                    tags: names,
                });
            }
            state.toast(
                strings::library::bulk_changed(games.len()),
                ToastKind::Success,
                None,
                env.mono,
            );
        }
        BulkAction::Favorite => {
            for g in &games {
                env.bridge.send(UiCommand::SetFavorite {
                    id: g.id,
                    favorite: true,
                });
            }
            state.toast(
                strings::library::bulk_changed(games.len()),
                ToastKind::Success,
                None,
                env.mono,
            );
        }
        BulkAction::Hide => {
            for g in &games {
                env.bridge.send(UiCommand::SetHidden {
                    id: g.id,
                    hidden: true,
                });
            }
            state.toast(
                strings::library::bulk_changed(games.len()),
                ToastKind::Success,
                None,
                env.mono,
            );
            state.library.selection.clear();
        }
        BulkAction::Preset(p) => {
            if games.iter().any(|g| widens(g, p)) {
                state.show_dialog(Dialog::Confirm(Confirm::WidenProtection { ids, preset: p }));
            } else {
                env.bridge.send(UiCommand::ApplyPreset { ids, preset: p });
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
        R::DownloadRuntime => env.bridge.send(UiCommand::ProvisionRuntime {
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
        R::ViewLog => env.bridge.send(UiCommand::ViewLog {
            session: err.session,
            game,
        }),
        R::OpenSessionFolder => {
            if let Some(s) = err.session {
                env.bridge
                    .send(UiCommand::OpenFolder(FolderTarget::Session(s)));
            }
        }
        R::CopyDetails => {
            env.ctx.copy_text(strings::errors::copy_text(err));
            state.toast(
                strings::feedback::COPIED,
                ToastKind::Success,
                None,
                env.mono,
            );
        }
        R::RestoreBackup | R::OpenBackupsFolder | R::FreeSpaceInfo | R::CreateReport => {
            env.bridge
                .send(UiCommand::OpenFolder(FolderTarget::DataFolder));
        }
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
