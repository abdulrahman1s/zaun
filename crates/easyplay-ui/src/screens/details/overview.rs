//! Overview tab (SCREEN_SPECS §4): one sheet read from the top. *How it runs*: protection
//! with what the game can reach, the compatibility layer, the launch options, each row a way
//! into its tab, and the game's other programs to run. *Where it lives*: the game folder and
//! the save data, each row opening its folder. Then the notes (autosaved 800 ms after the
//! last keystroke). Playtime is beside Play, in the hero.

use std::time::Duration;

use easyplay_core::ids::GameId;
use easyplay_core::model::{ExeRole, GameSummary, SaveLocationKind, join_args};
use easyplay_core::sandbox::StatementSeverity;
use easyplay_core::stats::StatsRequest;
use egui::{Align, Layout, Ui, vec2};

use super::statement_icon;
use crate::app::UiState;
use crate::bridge::{FolderTarget, GameEdit, UiCommand};
use crate::screens::Env;
use crate::strings;
use crate::strings::details as s;
use crate::theme::tokens::space;
use crate::theme::{self, Type, icons};
use crate::vm::AsyncValue;
use crate::vm::details::{DetailsTab, range_for};
use crate::vm::dialogs::AfterDiscard;
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::link_row::{LinkRow, Point, Points, Tone, control_row, link_rows};
use crate::widgets::shield;
use crate::widgets::tooltip;

/// Access statements shown on the sheet; the Protection tab has every one.
const REACH_SHOWN: usize = 6;
/// Other programs offered on the sheet; the Launch options tab has every one.
const PROGRAMS_SHOWN: usize = 3;
/// Index of the 7-day range among the Activity ranges.
const WEEK: usize = 0;

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

/// Keep the last seven days of `g` current: asked for when the page opens and again
/// whenever a session has been counted. The week on screen stays until its successor
/// arrives.
pub fn keep_week(state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let stamp = (g.session_count, g.total_playtime_ms);
    if state.details.week_q.is_some() || state.details.week_for == Some(stamp) {
        return;
    }
    let Some(range) = range_for(WEEK, env.now, env.utc_offset_s) else {
        return;
    };
    let q = env.bridge.query_id();
    state.details.week_q = Some(q);
    state.details.week_for = Some(stamp);
    if state.details.week.ready().is_none() {
        state.details.week = AsyncValue::Loading { since: env.mono };
    }
    env.bridge.send(UiCommand::QueryStats {
        request: StatsRequest::GameOverview { game: g.id, range },
        reply_to: q,
    });
}

fn heading(ui: &mut Ui, title: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let h = ui.label(Type::Heading.rich(title).color(p.text_primary));
    crate::a11y::labelled(ui.ctx(), h.id, egui::Role::Heading, title);
    ui.add_space(space::S1);
}

fn tone(severity: StatementSeverity) -> Tone {
    match severity {
        StatementSeverity::Warning => Tone::Warning,
        StatementSeverity::Danger => Tone::Danger,
        _ => Tone::Normal,
    }
}

/// *How it runs*: the three settings tabs as rows, and the other programs.
fn runs(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    heading(ui, s::RUNS);
    if matches!(state.details.detail, AsyncValue::Failed(_)) {
        ui.horizontal(|ui| {
            ui.label(Type::Body.rich(s::COULDNT_LOAD).color(p.text_secondary));
            if ui.add(Button::new(s::RETRY).size(Size::Sm)).clicked() {
                super::load(state, env, g.id);
            }
        });
    }

    // Protection: the preset, then what it lets the game reach.
    let editor = state.details.protection.as_ref();
    let preset = editor.map_or(g.preset, |e| e.draft.preset);
    let attention = g.flags.needs_attention || g.flags.suspected_tamper;
    let preset_value = match preset {
        easyplay_core::model::Preset::Custom { base } => s::custom_based_on(base),
        other => strings::shield::preset_name(other).to_owned(),
    };
    let statements = editor.map(|e| e.assessment.statements.as_slice());
    let items: Vec<Point<'_>> = statements
        .unwrap_or_default()
        .iter()
        .take(REACH_SHOWN)
        .map(|st| Point {
            icon: statement_icon(st),
            text: &st.text,
            tone: tone(st.severity),
        })
        .collect();
    let more = statements
        .map(<[_]>::len)
        .filter(|n| *n > REACH_SHOWN)
        .map(|n| s::more_statements(n - REACH_SHOWN));
    let points = match statements {
        Some(_) => Points::Listed {
            name: s::REACH,
            items: &items,
            more: more.as_deref(),
        },
        None if state.details.detail.is_loading() => Points::Pending {
            lines: REACH_SHOWN / 2,
            skeleton: state.details.detail.show_skeleton(env.mono),
        },
        None => Points::None,
    };
    let attention_note = attention.then_some(strings::shield::ATTENTION);
    let protection_name = s::row_name(s::PROTECTION, &preset_value, attention_note);

    // Compatibility: the layer, and whether it is still there.
    let record = state.details.record();
    let layer_note = record.and_then(|d| {
        if d.runtime_missing {
            Some(s::LAYER_MISSING)
        } else {
            None
        }
    });
    let compat_title = s::tab(DetailsTab::Compatibility);
    let compat_name = s::row_name(compat_title, &g.runtime_label, layer_note);

    // Launch options: the program Play starts, and what the options add to it.
    let program = g.primary_exe_path.file_name().map_or_else(
        || g.primary_exe_path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let launch_note = record.and_then(|d| {
        let set = d
            .game
            .launch
            .env
            .iter()
            .filter(|v| !v.name.is_empty())
            .count();
        s::launch_summary(&join_args(&d.game.launch.args), set)
    });
    let launch_title = s::tab(DetailsTab::LaunchOptions);
    let launch_name = s::row_name(launch_title, &program, launch_note.as_deref());

    let warn = |on: bool| if on { Tone::Warning } else { Tone::Normal };
    let rows = [
        LinkRow::new(
            shield::glyph(preset, attention),
            s::PROTECTION,
            &preset_value,
            &protection_name,
        )
        .icon_tone(if attention {
            Tone::Danger
        } else {
            Tone::Normal
        })
        .detail(attention_note, Tone::Danger)
        .points(points),
        LinkRow::new(icons::LAYERS, compat_title, &g.runtime_label, &compat_name)
            .icon_tone(warn(layer_note.is_some()))
            .detail(layer_note, Tone::Warning),
        LinkRow::new(icons::TERMINAL, launch_title, &program, &launch_name)
            .detail(launch_note.as_deref(), Tone::Normal),
    ];
    let to = [
        DetailsTab::Protection,
        DetailsTab::Compatibility,
        DetailsTab::LaunchOptions,
    ];
    let chosen = link_rows(ui, egui::Id::unique(("details.runs", g.id)), &rows);

    // The game's other programs, each one press from running.
    let mut others: Vec<(easyplay_core::ids::ExecutableId, String, String)> = record
        .map(|d| {
            let mut exes: Vec<_> = d
                .game
                .executables
                .iter()
                .filter(|e| e.role != ExeRole::Primary)
                .collect();
            exes.sort_by_key(|e| (e.role.as_str(), e.path.clone()));
            exes.into_iter()
                .map(|e| {
                    let file = e
                        .path
                        .file_name()
                        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                    let label = e.label.clone().unwrap_or_else(|| file.clone());
                    (e.id, label, file)
                })
                .collect()
        })
        .unwrap_or_default();
    let hidden = others.len().saturating_sub(PROGRAMS_SHOWN);
    others.truncate(PROGRAMS_SHOWN);
    let mut run = None;
    let mut all_programs = false;
    if !others.is_empty() {
        control_row(ui, icons::FILE_EXE, s::OTHER_PROGRAMS, false, |ui| {
            for (exe, label, file) in &others {
                let name = s::run_program(file);
                let b = Button::new(label)
                    .size(Size::Sm)
                    .icon(icons::PLAY)
                    .accessible_name(name.clone());
                if tooltip::attach(ui.add(b), &name).clicked() {
                    run = Some(*exe);
                }
            }
            if hidden > 0 {
                let more = s::more_programs(hidden);
                all_programs = ui.add(Button::new(&more).variant(Variant::Link)).clicked();
            }
        });
    }
    if let Some(exe) = run {
        env.bridge.send(UiCommand::Launch {
            id: g.id,
            exe: Some(exe),
        });
    }
    let tab = chosen
        .map(|i| to[i])
        .or(all_programs.then_some(DetailsTab::LaunchOptions));
    if let Some(tab) = tab {
        state.navigate(AfterDiscard::Tab(tab.index()));
    }
}

/// *Where it lives*: the two folders, each row opening its own.
fn lives(ui: &mut Ui, state: &UiState, env: &mut Env<'_>, g: &GameSummary) {
    heading(ui, s::LIVES);
    let (folder, parent) = s::location(g.root.as_deref(), &g.primary_exe_path);
    let folder_title = if g.root.is_some() {
        s::GAME_FOLDER
    } else {
        s::PROGRAM_FILE
    };
    let folder_name = s::row_name(folder_title, &folder, parent.as_deref());
    let (save, save_folder) = match g.save_location {
        SaveLocationKind::Internal => (s::PRIVATE_STORAGE, None),
        SaveLocationKind::External => (
            s::YOUR_FOLDER,
            state
                .details
                .record()
                .and_then(|d| d.save_location.as_deref())
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned()),
        ),
    };
    let save_name = s::row_name(s::SAVE_DATA, save, save_folder.as_deref());
    let rows = [
        LinkRow::new(icons::FOLDER, folder_title, &folder, &folder_name)
            .detail(parent.as_deref(), Tone::Normal)
            .external(s::OPEN_IN_FILES, Some("Ctrl+O")),
        LinkRow::new(icons::FLOPPY, s::SAVE_DATA, save, &save_name)
            .detail(save_folder.as_deref(), Tone::Normal)
            .external(s::OPEN_IN_FILES, None),
    ];
    match link_rows(ui, egui::Id::unique(("details.lives", g.id)), &rows) {
        Some(0) => env
            .bridge
            .send(UiCommand::OpenFolder(FolderTarget::GameFolder(g.id))),
        Some(_) => env
            .bridge
            .send(UiCommand::OpenFolder(FolderTarget::SaveData(g.id))),
        None => {}
    }
}

fn notes(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.horizontal(|ui| {
        heading(ui, s::NOTES);
        if state
            .details
            .notes_saved
            .is_some_and(|t| env.mono.saturating_duration_since(t) < Duration::from_secs(2))
        {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    Type::Caption
                        .rich(strings::feedback::SAVED)
                        .color(p.text_secondary),
                );
                ui.label(icons::icon(icons::CHECK, theme::tokens::icon::INLINE).color(p.success));
            });
        }
    });
    ui.add_space(space::S2);
    let rows = if state.details.notes.is_empty() { 1 } else { 3 };
    // The notes arrive with the record: until then there is nothing to add to, and typing
    // would write over what was not shown.
    let r = ui.add_enabled(
        state.details.record().is_some(),
        egui::TextEdit::multiline(&mut state.details.notes)
            .hint_text(s::ADD_NOTE)
            .desired_rows(rows)
            .desired_width(f32::INFINITY)
            .margin(vec2(space::S3, space::S2)),
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
}

/// Show the Overview tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width();
    ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.set_width(w);
        crate::screens::debug::advanced_note(ui, state, env);
        ui.spacing_mut().item_spacing.y = space::S1;
        runs(ui, state, env, g);
        ui.add_space(space::S6);
        lives(ui, state, env, g);
        ui.add_space(space::S6);
        notes(ui, state, env, g);
        ui.add_space(space::S4);
        let date = strings::fmt::relative_inline(g.added_at, env.now, env.utc_offset_s);
        ui.label(Type::Caption.numeric(s::added(&date)).color(p.text_muted));
    });
}
