//! Overview tab (SCREEN_SPECS §4): what the game can reach, playtime with a mini chart,
//! details, recent sessions and notes (autosaved 800 ms after the last keystroke).

use std::time::Duration;

use easyplay_core::ids::GameId;
use easyplay_core::model::GameSummary;
use easyplay_core::stats::StatsRequest;
use egui::Ui;

use super::{card, skeleton_lines, statement_row};
use crate::app::UiState;
use crate::bridge::{FolderTarget, GameEdit, UiCommand};
use crate::screens::Env;
use crate::strings;
use crate::strings::details as s;
use crate::theme::tokens::{layout, space};
use crate::theme::{self, Type, icons};
use crate::vm::AsyncValue;
use crate::vm::details::{DetailsTab, range_for};
use crate::widgets::bar_chart::{ChartOpts, bar_chart};
use crate::widgets::button::{Button, Size, Variant};

/// Ask for the playtime overview of the Activity range (7 days by default).
pub fn request(state: &mut UiState, env: &mut Env<'_>, id: GameId) {
    let Some(range) = range_for(state.details.activity.range, env.now, env.utc_offset_s) else {
        return;
    };
    let q = env.bridge.query_id();
    state.details.activity.overview_q = Some(q);
    state.details.activity.overview = AsyncValue::Loading { since: env.mono };
    env.bridge.send(UiCommand::QueryStats {
        request: StatsRequest::GameOverview { game: id, range },
        reply_to: q,
    });
}

fn access_card(ui: &mut Ui, state: &mut UiState, env: &Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    card(ui, s::REACH, |ui| {
        ui.horizontal(|ui| {
            crate::widgets::shield::badge(
                ui,
                g.preset,
                g.flags.needs_attention,
                theme::tokens::icon::NAV,
            );
            ui.label(
                Type::BodyStrong
                    .rich(strings::shield::preset_name(g.preset))
                    .color(p.text_primary),
            );
        });
        match &state.details.protection {
            Some(e) => {
                let list = ui.scope(|ui| {
                    for st in e.assessment.statements.iter().take(6) {
                        statement_row(ui, st, None);
                    }
                });
                crate::a11y::labelled(ui.ctx(), list.response.id, egui::Role::List, s::REACH);
            }
            None if state.details.detail.show_skeleton(env.mono) => skeleton_lines(ui, 4),
            None => {}
        }
        if ui
            .add(Button::new(s::SEE_EVERYTHING).variant(Variant::Link))
            .clicked()
        {
            state.details.tab = DetailsTab::Protection;
        }
    });
}

fn playtime_card(ui: &mut Ui, state: &mut UiState, env: &Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    card(ui, s::PLAYTIME, |ui| {
        if g.total_playtime_ms == 0 {
            ui.label(
                Type::Body
                    .rich(strings::card::NOT_PLAYED)
                    .color(p.text_secondary),
            );
            return;
        }
        ui.horizontal(|ui| {
            ui.label(
                Type::Title
                    .numeric(strings::fmt::duration(g.total_playtime_ms))
                    .color(p.text_primary),
            );
            ui.label(
                Type::Body
                    .numeric(s::sessions(g.session_count))
                    .color(p.text_secondary),
            );
        });
        match &state.details.activity.overview {
            AsyncValue::Ready(o) => {
                let bars = crate::screens::statistics::bars(&o.series, env.utc_offset_s);
                bar_chart(
                    ui,
                    s::LAST_7_DAYS,
                    &bars,
                    ChartOpts {
                        height: space::S12,
                        axes: false,
                    },
                    &mut state.details.activity.bar,
                );
                ui.label(Type::Caption.rich(s::LAST_7_DAYS).color(p.text_muted));
            }
            v if v.show_skeleton(env.mono) => skeleton_lines(ui, 2),
            AsyncValue::Failed(_) => {
                ui.label(Type::Caption.rich(s::COULDNT_LOAD).color(p.text_muted));
            }
            _ => {}
        }
    });
}

fn details_card(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    card(ui, s::DETAILS, |ui| {
        egui::Grid::new("details.grid")
            .num_columns(2)
            .spacing(egui::vec2(space::S4, space::S2))
            .show(ui, |ui| {
                ui.label(Type::Body.rich(s::LOCATION).color(p.text_muted));
                ui.horizontal_wrapped(|ui| {
                    let (name, parent) = s::location(g.root.as_deref(), &g.primary_exe_path);
                    ui.label(Type::BodyStrong.rich(name).color(p.text_primary));
                    if let Some(parent) = parent {
                        ui.label(Type::Body.rich(parent).color(p.text_secondary));
                    }
                    if ui
                        .add(Button::ghost(s::OPEN).size(Size::Sm).icon(icons::OPEN))
                        .clicked()
                    {
                        env.bridge
                            .send(UiCommand::OpenFolder(FolderTarget::GameFolder(g.id)));
                    }
                });
                ui.end_row();
                ui.label(Type::Body.rich(s::SAVE_DATA).color(p.text_muted));
                ui.horizontal(|ui| {
                    let label = match g.save_location {
                        easyplay_core::model::SaveLocationKind::Internal => s::PRIVATE_STORAGE,
                        easyplay_core::model::SaveLocationKind::External => s::YOUR_FOLDER,
                    };
                    ui.label(Type::Body.rich(label).color(p.text_primary));
                    if ui
                        .add(Button::ghost(s::OPEN).size(Size::Sm).icon(icons::OPEN))
                        .clicked()
                    {
                        env.bridge
                            .send(UiCommand::OpenFolder(FolderTarget::SaveData(g.id)));
                    }
                });
                ui.end_row();
                ui.label(Type::Body.rich(s::ADDED).color(p.text_muted));
                ui.label(
                    Type::Body
                        .numeric(strings::fmt::relative_date(
                            g.added_at,
                            env.now,
                            env.utc_offset_s,
                        ))
                        .color(p.text_primary),
                );
                ui.end_row();
                if let Some(d) = state.details.record() {
                    ui.label(Type::Body.rich(s::PROGRAMS).color(p.text_muted));
                    ui.label(
                        Type::Body
                            .numeric(d.game.executables.len().to_string())
                            .color(p.text_primary),
                    );
                    ui.end_row();
                }
            });
    });
}

fn sessions_card(ui: &mut Ui, state: &mut UiState, env: &Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let AsyncValue::Ready(o) = &state.details.activity.overview else {
        return;
    };
    if o.recent.is_empty() {
        return;
    }
    let rows = o.recent.clone();
    card(ui, s::RECENT_SESSIONS, |ui| {
        egui::Grid::new("details.recent")
            .num_columns(3)
            .spacing(egui::vec2(space::S4, space::S2))
            .show(ui, |ui| {
                for r in rows.iter().take(5) {
                    ui.label(
                        Type::Body
                            .numeric(strings::fmt::relative_date(
                                r.started_at,
                                env.now,
                                env.utc_offset_s,
                            ))
                            .color(p.text_secondary),
                    );
                    ui.label(
                        Type::Body
                            .numeric(strings::fmt::duration(r.duration_ms))
                            .color(p.text_primary),
                    );
                    ui.label(
                        Type::Body
                            .rich(
                                r.end_reason
                                    .map_or(strings::launch::RUNNING, strings::launch::ended),
                            )
                            .color(p.text_muted),
                    );
                    ui.end_row();
                }
            });
        if ui
            .add(Button::new(s::SEE_ALL).variant(Variant::Link))
            .clicked()
        {
            state.details.tab = DetailsTab::Activity;
        }
    });
}

fn notes_card(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    card(ui, s::NOTES, |ui| {
        let rows = if state.details.notes.is_empty() { 1 } else { 4 };
        let r = ui.add(
            egui::TextEdit::multiline(&mut state.details.notes)
                .hint_text(s::ADD_NOTE)
                .desired_rows(rows)
                .desired_width(f32::INFINITY),
        );
        crate::a11y::describe(&r, s::NOTES);
        if r.changed() {
            state.details.notes_edited = Some(env.mono);
        }
        if let Some(t) = state.details.notes_edited {
            let due = Duration::from_millis(800);
            let since = env.mono.saturating_duration_since(t);
            if since >= due {
                state.details.notes_edited = None;
                state.details.notes_saved = Some(env.mono);
                env.bridge.send(UiCommand::UpdateGame {
                    id: g.id,
                    edits: vec![GameEdit::Notes(state.details.notes.clone())],
                });
            } else {
                ui.ctx().request_repaint_after(due - since);
            }
        }
        if state
            .details
            .notes_saved
            .is_some_and(|t| env.mono.saturating_duration_since(t) < Duration::from_secs(2))
        {
            ui.horizontal(|ui| {
                ui.label(icons::icon(icons::CHECK, theme::tokens::icon::INLINE).color(p.success));
                ui.label(
                    Type::Caption
                        .rich(strings::feedback::SAVED)
                        .color(p.text_secondary),
                );
            });
        }
    });
}

/// Show the Overview tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    crate::screens::debug::advanced_note(ui, state, env);
    let wide = ui.available_width() >= layout::COMPACT_BELOW;
    if wide {
        ui.columns(2, |cols| {
            cols[0].spacing_mut().item_spacing.y = space::S4;
            cols[1].spacing_mut().item_spacing.y = space::S4;
            access_card(&mut cols[0], state, env, g);
            sessions_card(&mut cols[0], state, env);
            notes_card(&mut cols[0], state, env, g);
            playtime_card(&mut cols[1], state, env, g);
            details_card(&mut cols[1], state, env, g);
        });
    } else {
        ui.spacing_mut().item_spacing.y = space::S4;
        access_card(ui, state, env, g);
        playtime_card(ui, state, env, g);
        details_card(ui, state, env, g);
        sessions_card(ui, state, env);
        notes_card(ui, state, env, g);
    }
}
