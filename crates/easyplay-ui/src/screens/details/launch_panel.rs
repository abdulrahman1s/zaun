//! Launch progress and the problem panel (UI-016; USER_EXPERIENCE §7, UF-9, UF-11,
//! SCREEN_SPECS_2 §16): the state area under the hero, reused by the setup sheet after *Play*
//! and by the quick-launch window.

use easyplay_core::error::UserError;
use easyplay_core::ids::GameId;
use easyplay_core::model::{Activity, Availability, GameSummary};
use egui::{Align, CornerRadius, Layout, Ui};

use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::input::picker::PickPurpose;
use crate::screens::Env;
use crate::screens::actions;
use crate::strings;
use crate::strings::launch as l;
use crate::theme::palette::fade;
use crate::theme::tokens::{alpha, icon as icon_size, radius, space, stroke};
use crate::theme::{self, Icon, Type, icons};
use crate::vm::launch::{GameLaunch, present};
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::progress;

fn frame<R>(ui: &mut Ui, tint: egui::Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = theme::theme(ui.ctx()).palette.clone();
    egui::Frame::NONE
        .fill(fade(tint, alpha::BANNER_FILL))
        .stroke(egui::Stroke::new(stroke::BORDER, fade(tint, 0.4)))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(space::S4)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = space::S2;
            let _ = p;
            add(ui)
        })
        .inner
}

/// The progress row: phase, download bytes and rate, an indeterminate bar, *Cancel*.
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
    frame(ui, p.accent, |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(
                egui::vec2(icon_size::BUTTON, icon_size::BUTTON),
                egui::Sense::hover(),
            );
            progress::spinner(ui, r.center(), icon_size::BUTTON / 2.0 - 2.0, p.accent);
            let t = ui.label(Type::BodyStrong.rich(&pr.phase).color(p.text_primary));
            crate::a11y::labelled(ui.ctx(), t.id, egui::Role::Status, &pr.phase);
            if let Some(b) = pr.bytes {
                ui.label(
                    Type::Body
                        .numeric(l::bytes_rate(b, launch.rate))
                        .color(p.text_secondary),
                );
            }
            if pr.cancellable {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(Button::new(l::CANCEL).size(Size::Sm)).clicked() {
                        cancel = true;
                    }
                });
            }
        });
        progress::linear(ui, None, l::PROGRESS);
        if pr.offline_note {
            ui.label(Type::Caption.rich(l::OFFLINE_NOTE).color(p.text_secondary));
        }
        if pr.reassure {
            ui.label(Type::Caption.rich(l::REASSURE).color(p.text_secondary));
        }
    });
    cancel
}

/// The problem panel for `game`: the diagnosis, up to three suggestions, *Try again*,
/// *View log*, *Copy details*, the code in muted text.
pub fn problem_panel(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    game: GameId,
    err: Option<UserError>,
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let suggestions = state.launches.suggestions(game, err.as_ref());
    let fallback = UserError {
        code: easyplay_core::error::ErrorCode::UmuProgramDidNotStart,
        severity: easyplay_core::error::Severity::Error,
        title: l::CLOSED_EARLY.to_owned(),
        message: l::CLOSED_EARLY_BODY.to_owned(),
        actions: suggestions.clone(),
        details: None,
        game: Some(game),
        session: None,
    };
    let e = err.unwrap_or(fallback);
    let mut chosen = None;
    let mut retry = false;
    let mut dismiss = false;
    frame(ui, p.danger, |ui| {
        ui.horizontal(|ui| {
            ui.label(icons::icon(icons::WARNING, icon_size::BUTTON).color(p.danger));
            let t = ui.label(Type::Heading.rich(&e.title).color(p.text_primary));
            crate::a11y::labelled(ui.ctx(), t.id, egui::Role::Alert, &e.title);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(Type::Caption.rich(e.code.as_str()).color(p.text_muted));
            });
        });
        ui.add(egui::Label::new(Type::Body.rich(&e.message).color(p.text_secondary)).wrap());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
            for (i, a) in suggestions.iter().enumerate() {
                let v = if i == 0 {
                    Variant::Primary
                } else {
                    Variant::Secondary
                };
                if ui
                    .add(
                        Button::new(strings::errors::action(*a))
                            .variant(v)
                            .size(Size::Sm),
                    )
                    .clicked()
                {
                    chosen = Some(*a);
                }
            }
            if ui
                .add(
                    Button::new(l::TRY_AGAIN)
                        .size(Size::Sm)
                        .icon(icons::REFRESH),
                )
                .clicked()
            {
                retry = true;
            }
            if ui.add(Button::ghost(l::VIEW_LOG).size(Size::Sm)).clicked() {
                chosen = Some(easyplay_core::error::RecoveryAction::ViewLog);
            }
            if ui
                .add(
                    Button::ghost(l::COPY_DETAILS)
                        .size(Size::Sm)
                        .icon(icons::COPY),
                )
                .clicked()
            {
                chosen = Some(easyplay_core::error::RecoveryAction::CopyDetails);
            }
            if ui.add(Button::ghost(l::DISMISS).size(Size::Sm)).clicked() {
                dismiss = true;
            }
        });
    });
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

fn notice(
    ui: &mut Ui,
    tint: egui::Color32,
    icon: Icon,
    title: &str,
    body: &str,
    actions: &[&str],
) -> Option<usize> {
    let p = theme::theme(ui.ctx()).palette.clone();
    let mut out = None;
    frame(ui, tint, |ui| {
        ui.horizontal(|ui| {
            ui.label(icons::icon(icon, icon_size::BUTTON).color(tint));
            ui.vertical(|ui| {
                ui.label(Type::BodyStrong.rich(title).color(p.text_primary));
                ui.add(egui::Label::new(Type::Body.rich(body).color(p.text_secondary)).wrap());
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for (i, a) in actions.iter().enumerate().rev() {
                    if ui.add(Button::new(a).size(Size::Sm)).clicked() {
                        out = Some(i);
                    }
                }
            });
        });
    });
    out
}

/// The state area between the hero and the tabs: at most one item.
pub fn state_area(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let id = g.id;
    if let Some(launch) = state.launches.active(id).cloned()
        && !matches!(
            launch.state,
            easyplay_core::launch::LaunchState::Running { .. }
        )
    {
        if progress_row(ui, &launch, env.mono) {
            env.bridge.send(UiCommand::CancelLaunch { id });
        }
        ui.add_space(space::S4);
        return;
    }
    if state.launches.failures.contains_key(&id) || state.launches.ended_early.contains_key(&id) {
        let err = state.launches.failures.get(&id).cloned();
        problem_panel(ui, state, env, id, err);
        ui.add_space(space::S4);
        return;
    }
    match g.activity {
        Activity::Lingering => {
            if notice(
                ui,
                p.info,
                icons::CLOCK,
                l::BACKGROUND,
                l::BACKGROUND_BODY,
                &[l::FORCE_STOP],
            ) == Some(0)
            {
                env.bridge.send(UiCommand::Stop { id, force: true });
            }
            ui.add_space(space::S4);
            return;
        }
        Activity::Stopping => {
            if notice(
                ui,
                p.info,
                icons::STOP,
                strings::card::STOPPING,
                strings::details::STOPPING_BODY,
                &[l::FORCE_STOP],
            ) == Some(0)
            {
                env.bridge.send(UiCommand::Stop { id, force: true });
            }
            ui.add_space(space::S4);
            return;
        }
        _ => {}
    }
    match g.availability {
        Availability::Missing | Availability::DriveDisconnected | Availability::Unreadable => {
            let (title, body) = strings::details::availability(g.availability);
            match notice(
                ui,
                p.warning,
                icons::LINK_BREAK,
                title,
                body,
                &[strings::play::LOCATE, strings::details::CHECK_AGAIN],
            ) {
                Some(0) => state.picks.push(PickPurpose::Locate(id)),
                Some(1) => env.bridge.send(UiCommand::RescanMissing),
                _ => {}
            }
            ui.add_space(space::S4);
        }
        _ if g.flags.suspected_tamper => {
            notice(
                ui,
                p.warning,
                icons::SHIELD_ALERT,
                strings::details::TAMPER,
                strings::details::TAMPER_BODY,
                &[],
            );
            ui.add_space(space::S4);
        }
        _ => {}
    }
}
