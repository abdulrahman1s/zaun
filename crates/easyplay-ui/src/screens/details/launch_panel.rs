//! Launch progress and the problem note (UI-016; USER_EXPERIENCE §7, UF-9, UF-11,
//! SCREEN_SPECS_2 §16). On the game page a problem is a coral note above the tabs
//! (easyplay-redesign.html `notesHTML`): its title in bold with the code after it, what
//! happened, then ghost buttons: what helps (the catalogue's suggestions), *Try again*,
//! *View log*, *Dismiss*; the code itself copies the details. The setup sheet reuses the
//! progress row and the problem note after *Play*.

use easyplay_core::error::{RecoveryAction, UserError};
use easyplay_core::ids::GameId;
use egui::Ui;

use super::{Note, NoteTone, note};
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::screens::Env;
use crate::screens::actions;
use crate::strings;
use crate::strings::launch as l;
use crate::theme::{self, Type, icons};
use crate::vm::launch::{GameLaunch, present};
use crate::widgets::button::{Button, Size};
use crate::widgets::progress;

/// The progress row: the phase, download bytes and rate, an indeterminate bar, *Cancel*.
/// Returns true when *Cancel* was pressed.
pub fn progress_row(
    ui: &mut Ui,
    launch: &GameLaunch,
    now: easyplay_core::clock::MonoInstant,
) -> bool {
    let p = theme::theme(ui.ctx()).palette.clone();
    let elapsed = now.saturating_duration_since(launch.since).as_secs_f32();
    let pr = present(&launch.state, elapsed);
    let mut cancel = false;
    ui.horizontal(|ui| {
        let t = ui.label(Type::BodyStrong.rich(&pr.phase).color(p.t1));
        crate::a11y::labelled(ui.ctx(), t.id, egui::Role::Status, &pr.phase);
        if let Some(b) = pr.bytes {
            ui.label(
                Type::Body
                    .numeric(l::bytes_rate(b, launch.rate))
                    .color(p.t2),
            );
        }
        if pr.cancellable {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(Button::ghost(l::CANCEL()).size(Size::Md)).clicked() {
                    cancel = true;
                }
            });
        }
    });
    progress::linear(ui, None, l::PROGRESS());
    if pr.offline_note {
        ui.label(Type::Caption.rich(l::OFFLINE_NOTE()).color(p.t3));
    }
    if pr.reassure {
        ui.label(Type::Caption.rich(l::REASSURE()).color(p.t3));
    }
    cancel
}

/// The problem note of the game page.
pub fn problem_note(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, game: GameId) {
    let err = state.launches.failures.get(&game).cloned();
    problem_panel(ui, state, env, game, err);
}

/// The problem note for `game`: the diagnosis, its code, what helps, *Try again*, *View
/// log*, *Dismiss*; pressing the code copies the details.
pub fn problem_panel(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    game: GameId,
    err: Option<UserError>,
) {
    let suggestions = state.launches.suggestions(game, err.as_ref());
    let fallback = UserError {
        code: easyplay_core::error::ErrorCode::UmuProgramDidNotStart,
        severity: easyplay_core::error::Severity::Error,
        title: l::CLOSED_EARLY().to_owned(),
        message: l::CLOSED_EARLY_BODY().to_owned(),
        actions: suggestions.clone(),
        details: None,
        game: Some(game),
        session: None,
    };
    let e = err.unwrap_or(fallback);
    let mut labels: Vec<&str> = suggestions
        .iter()
        .map(|a| strings::errors::action(*a))
        .collect();
    let fixed = [l::TRY_AGAIN, l::VIEW_LOG, l::DISMISS];
    labels.extend(fixed.map(|f| f()));
    let out = note(
        ui,
        &Note {
            tone: NoteTone::Problem,
            icon: icons::WARNING,
            title: &e.title,
            code: Some(e.code.as_str()),
            text: &e.message,
            break_after_title: true,
            actions: &labels,
        },
    );
    let mut chosen = out.copy.then_some(RecoveryAction::CopyDetails);
    let mut retry = false;
    let mut dismiss = false;
    match out.action {
        Some(i) if i < suggestions.len() => chosen = Some(suggestions[i]),
        Some(i) => match i - suggestions.len() {
            0 => retry = true,
            1 => chosen = Some(RecoveryAction::ViewLog),
            _ => dismiss = true,
        },
        None => {}
    }
    if let Some(a) = chosen {
        actions::recover(state, env, &e, a);
    }
    if retry {
        state.launches.dismiss(game);
        env.bridge.send(UiCommand::Launch {
            id: game,
            exe: None,
        });
    }
    if dismiss {
        state.launches.dismiss(game);
    }
}
