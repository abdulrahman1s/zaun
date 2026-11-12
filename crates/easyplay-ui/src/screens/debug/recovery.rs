//! Recovery mode (ERROR_HANDLING §5, §4.12–4.13; PERSISTENCE §8; ERR-004). A damaged library
//! that salvage could not repair completely, or a library from a newer EasyPlay, replaces the
//! content area with a full-area state and its choices. A failed update shows a banner
//! instead (the library stays visible, read-only; see `super::banners`).

use egui::Ui;

use super::super::Env;
use crate::app::UiState;
use crate::bridge::{DatabaseChoice, FolderTarget, RecoveryReason, UiCommand};
use crate::strings::debug as s;
use crate::theme::tokens::space;
use crate::theme::{self, Type};
use crate::widgets::button::Button;

/// Draw the full-area state when one applies; returns whether it did.
pub fn full_area(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) -> bool {
    let diag = env.bridge.diagnostics().clone();
    let Some(r) = diag.recovery.as_ref() else {
        state.debug.recovery_sent = false;
        return false;
    };
    let p = theme::theme(ui.ctx()).palette.clone();
    let body = |ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>| match &r.reason {
        RecoveryReason::Damaged {
            backups,
            repaired_games,
            missing_games,
        } => {
            ui.label(Type::Title.rich(s::DAMAGED_TITLE).color(p.text_primary));
            ui.add_space(space::S2);
            ui.add(
                egui::Label::new(Type::Body.rich(s::DAMAGED_BODY).color(p.text_secondary)).wrap(),
            );
            ui.add_space(space::S4);
            // The choices, the default first: the newest backup.
            let mut choices: Vec<(String, Option<String>, DatabaseChoice)> = Vec::new();
            if let Some((path, at, games)) = backups.first() {
                choices.push((
                    s::restore_backup(
                        &crate::strings::fmt::relative_date(*at, env.now, env.utc_offset_s),
                        *games,
                    ),
                    None,
                    DatabaseChoice::RestoreBackup(path.clone()),
                ));
            }
            if let Some(n) = repaired_games {
                let missing = (!missing_games.is_empty())
                    .then(|| s::missing_games(&missing_games.join(", ")));
                choices.push((s::use_repaired(*n), missing, DatabaseChoice::UseRepaired));
            }
            choices.push((
                s::START_EMPTY.to_owned(),
                Some(s::START_EMPTY_NOTE.to_owned()),
                DatabaseChoice::StartEmpty,
            ));
            if backups.is_empty() {
                ui.label(Type::Caption.rich(s::NO_BACKUPS).color(p.text_muted));
            }
            let sel = &mut state.debug.recovery_choice;
            *sel = (*sel).min(choices.len() - 1);
            for (i, (label, note, _)) in choices.iter().enumerate() {
                ui.radio_value(sel, i, Type::BodyStrong.rich(label).color(p.text_primary));
                if let Some(n) = note {
                    ui.indent(("choice", i), |ui| {
                        ui.add(
                            egui::Label::new(Type::Caption.rich(n).color(p.text_secondary)).wrap(),
                        );
                    });
                }
            }
            ui.add_space(space::S4);
            ui.horizontal(|ui| {
                let busy = state.debug.recovery_sent;
                if ui.add(Button::primary(s::CONTINUE).busy(busy)).clicked() && !busy {
                    let c = choices[state.debug.recovery_choice].2.clone();
                    state.debug.recovery_sent = true;
                    env.bridge.send(UiCommand::ResolveDatabase(c));
                }
                if ui.add(Button::new(s::CREATE_REPORT)).clicked() {
                    super::open_report(state, env, None, None);
                }
                if ui.add(Button::ghost(s::QUIT)).clicked() {
                    env.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }
        RecoveryReason::NewerVersion | RecoveryReason::MigrationFailed { .. } => {
            ui.label(Type::Title.rich(&r.error.title).color(p.text_primary));
            ui.add_space(space::S2);
            ui.add(
                egui::Label::new(Type::Body.rich(&r.error.message).color(p.text_secondary)).wrap(),
            );
            ui.add_space(space::S4);
            ui.horizontal(|ui| {
                if ui.add(Button::new(s::OPEN_BACKUPS)).clicked() {
                    env.bridge
                        .send(UiCommand::OpenFolder(FolderTarget::Backups));
                }
                if ui.add(Button::new(s::CREATE_REPORT)).clicked() {
                    super::open_report(state, env, None, None);
                }
                if ui.add(Button::ghost(s::QUIT)).clicked() {
                    env.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
        }
    };
    if matches!(r.reason, RecoveryReason::MigrationFailed { .. }) {
        // The library stays visible, read-only, under a banner.
        return false;
    }
    let w = ui.available_width().min(640.0);
    ui.add_space(space::S12);
    ui.vertical_centered(|ui| {
        ui.set_max_width(w);
        egui::Frame::NONE
            .fill(p.bg_surface)
            .corner_radius(8)
            .inner_margin(egui::Margin::same(space::S6 as i8))
            .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    body(ui, state, env);
                });
            });
    });
    true
}
