//! Recovery mode (ERROR_HANDLING §5, §4.12–4.13; PERSISTENCE §8; ERR-004). A damaged library
//! that salvage could not repair completely, or a library from a newer EasyPlay, replaces the
//! page with an empty state and its choices: a 440 column in the middle of the page, a
//! display headline, a sentence, then (for a damaged library) one choice row each for
//! restoring the newest backup, using the repaired library and starting empty, and the
//! actions *Continue*, *Create a report…* and *Quit*. A failed update shows a banner instead
//! (the library stays visible, read-only; see `super::banners`).

use egui::{Align, Layout, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use super::super::Env;
use crate::app::UiState;
use crate::bridge::{Command, DatabaseChoice, FolderRequest, RecoveryReason};
use crate::strings::debug as s;
use crate::theme::tokens::{control, layout, space};
use crate::theme::{self, Type};
use crate::widgets::button::{Button, Variant};
use crate::widgets::choice::{PickKind, pick};
use crate::widgets::tooltip;

/// One choice of the damaged library.
struct Choice {
    title: &'static str,
    note: String,
    /// What it sends; `None` when it cannot be taken (no usable backup).
    choice: Option<DatabaseChoice>,
    /// More about it, as the row's tooltip (the games repair could not read).
    more: Option<String>,
}

/// Draw the full-area state when one applies; returns whether it did.
pub fn full_area(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) -> bool {
    let diag = env.bridge.diagnostics().clone();
    let Some(r) = diag.recovery.as_ref() else {
        state.debug.recovery_sent = false;
        return false;
    };
    if matches!(r.reason, RecoveryReason::MigrationFailed { .. }) {
        // The library stays visible, read-only, under a banner.
        return false;
    }
    let gutter = super::super::shell::gutter(ui.ctx());
    egui::ScrollArea::vertical()
        .id_salt("recovery")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin {
                    left: gutter as i8,
                    right: gutter as i8,
                    top: (layout::PAGE_PAD[0] + layout::BLANK_PAD) as i8,
                    bottom: layout::BLANK_PAD as i8,
                })
                .show(ui, |ui| {
                    let w = ui.available_width().min(layout::BLANK_W);
                    let x = ui.cursor().min.x + (ui.available_width() - w) / 2.0;
                    let top = ui.cursor().min.y;
                    let mut col = ui.new_child(
                        UiBuilder::new()
                            .max_rect(Rect::from_min_size(pos2(x, top), vec2(w, f32::INFINITY)))
                            .layout(Layout::top_down(Align::Min)),
                    );
                    col.set_width(w);
                    col.spacing_mut().item_spacing.y = 0.0;
                    match &r.reason {
                        RecoveryReason::Damaged {
                            backups,
                            repaired_games,
                            missing_games,
                        } => damaged(
                            &mut col,
                            state,
                            env,
                            backups,
                            *repaired_games,
                            missing_games,
                        ),
                        _ => newer(&mut col, state, env, &r.error.title, &r.error.message),
                    }
                    let h = col.min_rect().height();
                    ui.allocate_rect(
                        Rect::from_min_size(
                            pos2(ui.cursor().min.x, top),
                            vec2(ui.available_width(), h),
                        ),
                        Sense::hover(),
                    );
                });
        });
    true
}

/// The headline and the sentence of an empty state.
fn heading(ui: &mut Ui, title: &str, body: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width();
    let g = Type::Blank.galley(ui, title, p.t1, w, usize::MAX);
    let (r, resp) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    ui.painter().galley(r.min, g, p.t1);
    crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Heading, title);
    ui.add_space(layout::LEDE_GAP);
    let g = Type::Lead.galley(ui, body, p.t2, w, usize::MAX);
    let (r, _) = ui.allocate_exact_size(vec2(w, g.size().y), Sense::hover());
    ui.painter().galley(r.min, g, p.t2);
}

fn damaged(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    backups: &[(std::path::PathBuf, easyplay_core::clock::Timestamp, u64)],
    repaired: Option<u64>,
    missing: &[String],
) {
    let p = theme::theme(ui.ctx()).palette.clone();
    heading(ui, s::DAMAGED_TITLE(), s::DAMAGED_BODY());
    ui.add_space(layout::PICKS_GAP);
    // The choices, the default first: the newest backup.
    let mut choices = vec![match backups.first() {
        Some((path, at, games)) => Choice {
            title: s::RESTORE_BACKUP(),
            note: s::backup_note(
                &crate::strings::stats::day(*at, env.now, env.utc_offset_s),
                *games,
            ),
            choice: Some(DatabaseChoice::RestoreBackup(path.clone())),
            more: None,
        },
        None => Choice {
            title: s::RESTORE_BACKUP(),
            note: s::NO_BACKUPS().to_owned(),
            choice: None,
            more: None,
        },
    }];
    if let Some(n) = repaired {
        choices.push(Choice {
            title: s::USE_REPAIRED(),
            note: s::repaired_note(n, missing.len()),
            choice: Some(DatabaseChoice::UseRepaired),
            more: (!missing.is_empty()).then(|| s::missing_games(&missing.join(", "))),
        });
    }
    choices.push(Choice {
        title: s::START_EMPTY(),
        note: s::START_EMPTY_NOTE().to_owned(),
        choice: Some(DatabaseChoice::StartEmpty),
        more: None,
    });
    let first_open = choices.iter().position(|c| c.choice.is_some()).unwrap_or(0);
    let sel = &mut state.debug.recovery_choice;
    if choices.get(*sel).is_none_or(|c| c.choice.is_none()) {
        *sel = first_open;
    }
    // The rows reach their padding past the column, so the radios stand on its axis.
    let pad = control::PICK_PAD[0];
    let w = ui.available_width() + pad * 2.0;
    let top = ui.cursor().min;
    let mut picks = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                pos2(top.x - pad, top.y),
                vec2(w, f32::INFINITY),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    picks.spacing_mut().item_spacing.y = 0.0;
    picks.set_width(w);
    let group = picks
        .scope(|ui| {
            for (i, c) in choices.iter().enumerate() {
                ui.add_enabled_ui(c.choice.is_some(), |ui| {
                    // Read whole: the choice, what it holds, and the games it lost.
                    let name = match &c.more {
                        Some(m) => s::choice_spoken(&[c.title, &c.note, m]),
                        None => s::choice_spoken(&[c.title, &c.note]),
                    };
                    let (resp, ()) = pick(ui, PickKind::Radio, *sel == i, &name, |ui| {
                        ui.label(Type::Body.rich(c.title).color(p.t1));
                        Type::Caption.paragraph(ui, &c.note, p.t3);
                    });
                    let resp = match &c.more {
                        Some(m) => tooltip::tooltip_at_pointer(resp, m, None),
                        None => resp,
                    };
                    if resp.clicked() && c.choice.is_some() {
                        *sel = i;
                    }
                });
            }
        })
        .response;
    crate::a11y::labelled(
        ui.ctx(),
        group.id,
        egui::Role::RadioGroup,
        s::DAMAGED_TITLE(),
    );
    let h = picks.min_rect().height();
    ui.allocate_rect(
        Rect::from_min_size(top, vec2(ui.available_width(), h)),
        Sense::hover(),
    );
    ui.add_space(layout::BLANK_ACTIONS_GAP);
    let chosen = choices
        .get(state.debug.recovery_choice)
        .and_then(|c| c.choice.clone());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        let busy = state.debug.recovery_sent;
        if ui.add(Button::primary(s::CONTINUE()).busy(busy)).clicked()
            && !busy
            && let Some(c) = chosen
        {
            state.debug.recovery_sent = true;
            env.bridge.send(Command::ResolveDatabase(c));
        }
        if ui
            .add(Button::new(s::CREATE_REPORT()).variant(Variant::Secondary))
            .clicked()
        {
            super::open_report(state, env, None, None);
        }
        if ui.add(Button::ghost(s::QUIT())).clicked() {
            env.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}

/// A library written by a newer EasyPlay: what happened, and where the backups are.
fn newer(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, title: &str, message: &str) {
    heading(ui, title, message);
    ui.add_space(layout::BLANK_ACTIONS_GAP);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        if ui
            .add(Button::new(s::OPEN_BACKUPS()).variant(Variant::Secondary))
            .clicked()
        {
            env.bridge.send(Command::OpenFolder(FolderRequest::Backups));
        }
        if ui
            .add(Button::new(s::CREATE_REPORT()).variant(Variant::Secondary))
            .clicked()
        {
            super::open_report(state, env, None, None);
        }
        if ui.add(Button::ghost(s::QUIT())).clicked() {
            env.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}
