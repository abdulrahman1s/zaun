//! Quick-launch layout (SCREEN_SPECS §7, UI-026, UF-3): the compact window shown when a known
//! game is opened from the file manager. Name and phase from the first frame; closes itself
//! 600 ms after the game runs; closing the window never cancels the launch, *Cancel* does.
//! *Open EasyPlay* grows the same window into the full layout. An unknown program opens the
//! setup sheet, and the window grows to fit it.

use std::time::Duration;

use egui::{Align, CornerRadius, Key, Layout, Modifiers, Sense, Ui, ViewportCommand, vec2};

use super::Env;
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::route::{Overlay, Route};
use crate::strings;
use crate::strings::setup as s;
use crate::theme::tokens::{layout, radius, space};
use crate::theme::{self, Type};
use crate::vm::dialogs::Dialog;
use crate::vm::launch::present;
use crate::widgets::button::{Button, Size};
use crate::widgets::cover::{CoverSource, paint_cover};
use crate::widgets::progress;

/// The compact window size.
pub const SIZE: [f32; 2] = [420.0, 150.0];
/// The size while the setup sheet is shown.
pub const SETUP_SIZE: [f32; 2] = [720.0, 620.0];

/// Switch to the full layout (same window, saved geometry).
pub fn expand(state: &mut UiState, env: &Env<'_>) {
    if let Some(q) = &mut state.quick {
        q.expanded = true;
    }
    env.ctx.send_viewport_cmd(ViewportCommand::Resizable(true));
    env.ctx
        .send_viewport_cmd(ViewportCommand::MinInnerSize(layout::MIN_WINDOW.into()));
    env.ctx
        .send_viewport_cmd(ViewportCommand::InnerSize(layout::DEFAULT_WINDOW.into()));
    state.nav.go_to(Route::Library);
}

/// Show the compact layout.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let Some(q) = state.quick.clone() else { return };
    // The setup sheet for an unknown program needs room.
    if state.nav.top_overlay() == Some(Overlay::Setup) {
        let size = ui.ctx().content_rect().size();
        if size.x < SETUP_SIZE[0] {
            env.ctx
                .send_viewport_cmd(ViewportCommand::InnerSize(SETUP_SIZE.into()));
        }
    }
    let lib = env.bridge.library().clone();
    let game = q.game.and_then(|g| lib.get(g).cloned());
    let name = game.as_ref().map_or_else(
        || {
            q.path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        },
        |g| g.name.to_string(),
    );
    let launch = q.game.and_then(|g| state.launches.games.get(&g).cloned());
    let failure = q
        .game
        .and_then(|g| state.launches.failures.get(&g).cloned());
    // Auto-close 600 ms after the game runs.
    if let Some(t) = q.running_at {
        let since = env.mono.saturating_duration_since(t);
        let due = Duration::from_millis(600);
        if since >= due {
            env.ctx.send_viewport_cmd(ViewportCommand::Close);
        } else {
            ui.ctx().request_repaint_after(due - since);
        }
    }
    let cancellable = launch
        .as_ref()
        .is_some_and(|l| l.state.kind().cancellable());
    if state.nav.overlays().is_empty()
        && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
    {
        if cancellable && let Some(g) = q.game {
            env.bridge.send(UiCommand::CancelLaunch { id: g });
        } else {
            env.ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
    egui::Frame::NONE.inner_margin(space::S4).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(48.0, 72.0), Sense::hover());
            paint_cover(
                ui.painter(),
                rect,
                CornerRadius::same(radius::SM),
                CoverSource::Placeholder,
                &name,
                &theme,
            );
            ui.add_space(space::S3);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = space::S1;
                ui.add(
                    egui::Label::new(Type::Heading.rich(&name).color(p.text_primary)).truncate(),
                );
                let phase = match (&failure, &launch) {
                    (Some(e), _) => e.title.clone(),
                    (None, Some(l)) => {
                        present(
                            &l.state,
                            env.mono.saturating_duration_since(l.since).as_secs_f32(),
                        )
                        .phase
                    }
                    (None, None) => strings::launch::STARTING.to_owned(),
                };
                let ph = ui.add(
                    egui::Label::new(Type::Body.rich(&phase).color(if failure.is_some() {
                        p.danger
                    } else {
                        p.text_secondary
                    }))
                    .wrap(),
                );
                crate::a11y::labelled(ui.ctx(), ph.id, egui::Role::Status, &phase);
                if failure.is_none() && q.running_at.is_none() {
                    progress::linear(ui, None, strings::launch::PROGRESS);
                }
            });
        });
        ui.add_space(space::S2);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(e) = &failure {
                if ui
                    .add(Button::primary(strings::launch::TRY_AGAIN).size(Size::Sm))
                    .clicked()
                    && let Some(g) = q.game
                {
                    state.launches.dismiss(g);
                    env.bridge.send(UiCommand::Launch { id: g, exe: None });
                }
                if ui.add(Button::new(s::DETAILS).size(Size::Sm)).clicked() {
                    state.show_dialog(Dialog::Error(e.clone()));
                }
            } else if cancellable
                && ui
                    .add(Button::new(strings::launch::CANCEL).size(Size::Sm))
                    .clicked()
                && let Some(g) = q.game
            {
                env.bridge.send(UiCommand::CancelLaunch { id: g });
            }
            if ui
                .add(Button::ghost(s::OPEN_EASYPLAY).size(Size::Sm))
                .clicked()
            {
                expand(state, env);
            }
        });
    });
}
