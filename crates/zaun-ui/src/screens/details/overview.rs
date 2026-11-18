//! Overview tab (zaun-redesign.html `tabOverview`): four groups under headings.
//! *How it runs*: three doors (protection: "Offline" or "Online", then what the game is
//! allowed and what is blocked; the compatibility layer; the launch options), then the
//! game's other programs as small boxed buttons. *Where it lives*: two doors that open the
//! file manager (the game folder, the save data). *Notes*: a field that grows with its
//! text, saved half a second after typing stops ("Saved" stays 1.6 s at the heading's end).
//! *What it is* (ADR-0028): what Zaun knows of the game from its files; a fact with
//! nothing to say is left out.

use std::time::Duration;

use egui::text::LayoutJob;
use egui::{Color32, Ui};
use zaun_core::ids::{ExecutableId, GameId};
use zaun_core::model::{ExeRole, GameSummary, SaveLocationKind, join_args};
use zaun_core::stats::StatsRequest;

use crate::app::UiState;
use crate::bridge::{Command, FolderRequest, GameEdit, UiCommand};
use crate::screens::Env;
use crate::strings;
use crate::strings::details as s;
use crate::theme::tokens::{control, game as gm, timing};
use crate::theme::{self, Type, icons};
use crate::vm::AsyncValue;
use crate::vm::details::{DetailsTab, is_online, range_for, reach};
use crate::vm::dialogs::AfterDiscard;
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::rows::{self, Leads};
use crate::widgets::text_field::TextField;
use crate::widgets::tooltip;

/// Other programs offered on the sheet; the Launch options tab has every one.
const PROGRAMS_SHOWN: usize = 3;

/// Ask for the playtime overview of the Activity range (7 days by default).
pub fn request(state: &mut UiState, env: &mut Env<'_>, id: GameId) {
    let Some(range) = range_for(state.details.activity.range, env.now, env.utc_offset_s) else {
        return;
    };
    let q = env.bridge.query_id();
    state.details.activity.overview_q = Some(q);
    state.details.activity.overview = AsyncValue::Loading { since: env.mono };
    env.bridge.send(Command::QueryStats {
        request: StatsRequest::GameOverview { game: id, range },
        reply_to: q,
    });
}

/// Runs of text in one wrapped line: `(text, style, colour)`.
pub(super) fn runs(ui: &mut Ui, parts: &[(&str, Type, Color32)]) -> egui::Response {
    let mut job = LayoutJob::default();
    for (text, style, c) in parts {
        job.append(
            &theme::typography::shaped(text),
            0.0,
            style.format(*c, false),
        );
    }
    job.wrap.max_width = ui.available_width();
    ui.add(egui::Label::new(job).wrap())
}

/// One fact of *What it is*: its name, what is known, and whether it warns.
struct Fact {
    title: &'static str,
    value: String,
    warn: bool,
}

/// The facts the game's files and its record give, in reading order.
fn facts(state: &UiState, env: &Env<'_>, g: &GameSummary) -> Vec<Fact> {
    let mut out = Vec::new();
    let mut fact = |title, value: String, warn| {
        out.push(Fact { title, value, warn });
    };
    let about = state.details.about.ready();
    let record = state.details.record();
    if let Some(e) = about.and_then(|a| a.engine) {
        fact(s::ENGINE(), s::engine_name(e), false);
    }
    let primary = record.and_then(|d| {
        d.game
            .executables
            .iter()
            .find(|e| e.role == ExeRole::Primary)
    });
    if let Some(kind) = primary.and_then(|e| s::program_fact(&e.pe)) {
        fact(s::PROGRAM(), kind, false);
    }
    if let Some(pe) = primary.map(|e| &e.pe)
        && let Some(company) = pe.company.as_deref().filter(|c| !c.trim().is_empty())
    {
        let product = pe
            .product_name
            .as_deref()
            .filter(|p| !p.trim().is_empty() && !p.eq_ignore_ascii_case(&g.name));
        fact(s::MADE_BY(), s::made_by(company, product), false);
    }
    if let Some(h) = about.map(|a| &a.hints) {
        if !h.anti_cheat.is_empty() {
            fact(s::ANTI_CHEAT(), s::anti_cheat(&h.anti_cheat), true);
        }
        let mut services: Vec<String> = Vec::new();
        if h.steam_drm {
            services.push(s::NEEDS_STEAM().to_owned());
        } else if h.steam_api {
            services.push(s::USES_STEAM_API().to_owned());
        }
        if h.store_sdk {
            services.push(s::STORE_SDK().to_owned());
        }
        if !services.is_empty() {
            fact(s::STORE_SERVICES(), s::joined(&services), h.steam_drm);
        }
    }
    if let Some(d) = record
        && let Some(known) = d.fix_title.clone()
    {
        if d.runtime_without_fixes {
            fact(s::GAME_FIXES(), s::fixes_not_applied(&known), true);
        } else {
            fact(s::GAME_FIXES(), s::fixes_for(&known), false);
        }
    }
    fact(
        s::ADDED(),
        s::date_label(g.added_at, env.now, env.utc_offset_s),
        false,
    );
    out
}

/// *What it is*.
fn what_it_is(ui: &mut Ui, state: &UiState, env: &Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    rows::heading(ui, s::WHAT_IT_IS(), false);
    for f in facts(state, env, g) {
        let c = if f.warn { p.amber } else { p.t1 };
        rows::fact(ui, f.title, |ui| {
            let r = Type::Body.paragraph(ui, &f.value, c);
            crate::a11y::item(
                &r,
                egui::accesskit::Role::Label,
                &s::fact_name(f.title, &f.value),
            );
        });
    }
}

/// *How it runs*: the three tabs of settings as doors, and the other programs.
fn how_it_runs(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    rows::heading(ui, s::RUNS(), true);
    if matches!(state.details.detail, AsyncValue::Failed(_)) {
        ui.horizontal(|ui| {
            ui.label(Type::Body.rich(s::COULDNT_LOAD()).color(p.t2));
            if ui.add(Button::ghost(s::RETRY())).clicked() {
                super::load(state, env, g.id);
            }
        });
    }
    let mut to: Option<DetailsTab> = None;

    // Protection: whether it can go online, then what it is allowed and what is blocked.
    let policy = state
        .details
        .protection
        .as_ref()
        .map(|e| e.draft.clone())
        .or_else(|| state.details.record().map(|d| d.game.policy.clone()));
    let level = s::net_word(is_online(&state.details, g));
    let attention = (g.flags.needs_attention || g.flags.suspected_tamper)
        .then_some(strings::shield::ATTENTION());
    let lists = policy.as_ref().map(reach);
    let spoken = s::row_name(s::PROTECTION(), level, attention);
    let door = rows::door(ui, s::PROTECTION(), &spoken, Leads::Tab, |ui| {
        ui.label(Type::BodyStrong.rich(level).color(p.t1));
        if let Some(a) = attention {
            ui.label(Type::Body.rich(a).color(p.coral));
        }
        if let Some((can, cant)) = &lists {
            ui.add_space(gm::REACH_TOP);
            rows::reach(ui, s::CAN_USE(), can, s::CANT_USE(), cant);
        }
    });
    if door.clicked() {
        to = Some(DetailsTab::Protection);
    }

    // Compatibility: the layer, and whether it is still there.
    let missing = state.details.record().is_some_and(|d| d.runtime_missing);
    let spoken = s::row_name(
        s::tab(DetailsTab::Compatibility),
        &g.runtime_label,
        missing.then_some(s::LAYER_MISSING()),
    );
    let door = rows::door(
        ui,
        s::tab(DetailsTab::Compatibility),
        &spoken,
        Leads::Tab,
        |ui| {
            ui.label(Type::Body.rich(&*g.runtime_label).color(p.t1));
            if missing {
                ui.label(Type::Body.rich(s::LAYER_MISSING()).color(p.amber));
            }
        },
    );
    if door.clicked() {
        to = Some(DetailsTab::Compatibility);
    }

    // Launch options: the program Play starts with its arguments, and the variables.
    let program = g.primary_exe_path.file_name().map_or_else(
        || g.primary_exe_path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let (args, vars) = state.details.record().map_or((String::new(), 0), |d| {
        (
            join_args(&d.game.launch.args),
            d.game
                .launch
                .env
                .iter()
                .filter(|v| !v.name.is_empty())
                .count(),
        )
    });
    let command = s::command_line(&program, &args);
    let after = (vars > 0).then(|| s::variables_after(vars));
    let spoken = s::row_name(
        s::tab(DetailsTab::LaunchOptions),
        &command,
        (vars > 0).then(|| s::variables(vars)).as_deref(),
    );
    let door = rows::door(
        ui,
        s::tab(DetailsTab::LaunchOptions),
        &spoken,
        Leads::Tab,
        |ui| {
            let mut parts = vec![(command.as_str(), Type::Mono, p.t1)];
            if let Some(a) = &after {
                parts.push((a.as_str(), Type::Body, p.t3));
            }
            runs(ui, &parts);
        },
    );
    if door.clicked() {
        to = Some(DetailsTab::LaunchOptions);
    }

    // The game's other programs, each one press from running.
    let mut others: Vec<(ExecutableId, String, String)> = state
        .details
        .record()
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
    if !others.is_empty() {
        rows::fact(ui, s::OTHER_PROGRAMS(), |ui| {
            // The buttons sit on the value's line and reach past it above and below.
            let h = gm::TOOL_LINE;
            let (row, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
            let at = egui::Rect::from_center_size(
                row.center(),
                egui::vec2(row.width(), control::BUTTON_SM),
            );
            let mut line = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(at)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            line.spacing_mut().item_spacing.x = gm::TOOL_GAP;
            for (exe, label, file) in &others {
                let name = s::run_program(file);
                let b = Button::new(label)
                    .size(Size::Sm)
                    .icon(icons::PLAY)
                    .accessible_name(name.clone());
                if tooltip::attach(line.add(b), &name).clicked() {
                    run = Some(*exe);
                }
            }
            if hidden > 0 {
                let more = s::more_programs(hidden);
                if line
                    .add(Button::new(&more).variant(Variant::Link))
                    .clicked()
                {
                    to = Some(DetailsTab::LaunchOptions);
                }
            }
        });
    }
    if let Some(exe) = run {
        env.bridge.send(UiCommand::Launch {
            id: g.id,
            exe: Some(exe),
        });
    }
    if let Some(tab) = to {
        state.navigate(AfterDiscard::Tab(tab.index()));
    }
}

/// *Where it lives*: the two folders, each door opening its own.
fn where_it_lives(ui: &mut Ui, state: &UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    rows::heading(ui, s::LIVES(), false);
    let (folder, parent) = s::location(g.root.as_deref(), &g.primary_exe_path);
    let folder_title = if g.root.is_some() {
        s::GAME_FOLDER
    } else {
        s::PROGRAM_FILE
    };
    let spoken = s::row_name(folder_title(), &folder, parent.as_deref());
    let door = rows::door(ui, folder_title(), &spoken, Leads::Outside, |ui| {
        let after = parent.as_ref().map(|x| format!(" {x}"));
        let mut parts = vec![(folder.as_str(), Type::Body, p.t1)];
        if let Some(a) = &after {
            parts.push((a.as_str(), Type::Body, p.t3));
        }
        runs(ui, &parts);
    });
    if tooltip::tooltip(door, s::OPEN_IN_FILE_MANAGER(), None).clicked() {
        env.bridge
            .send(Command::OpenFolder(FolderRequest::GameFolder(g.id)));
    }
    let (save, save_folder) = match g.save_location {
        SaveLocationKind::Internal => (s::SAVE_PRIVATE(), None),
        SaveLocationKind::External => (
            s::YOUR_FOLDER(),
            state
                .details
                .record()
                .and_then(|d| d.save_location.as_deref())
                .and_then(|p| p.file_name())
                .map(|n| format!(" {}", n.to_string_lossy())),
        ),
    };
    let spoken = s::row_name(s::SAVE_DATA(), save, save_folder.as_deref().map(str::trim));
    let door = rows::door(ui, s::SAVE_DATA(), &spoken, Leads::Outside, |ui| {
        let mut parts = vec![(save, Type::Body, p.t1)];
        if let Some(a) = &save_folder {
            parts.push((a.as_str(), Type::Body, p.t3));
        }
        runs(ui, &parts);
    });
    if tooltip::tooltip(door, s::OPEN_IN_FILE_MANAGER(), None).clicked() {
        env.bridge
            .send(Command::OpenFolder(FolderRequest::SaveData(g.id)));
    }
}

/// *Notes*: saved half a second after typing stops; "Saved" then stays 1.6 s.
fn notes(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    // The notes arrive with the record: until then there is nothing to add to, and typing
    // would write over what was not shown.
    let ready = state.details.record().is_some();
    notes_field(ui, state, env, g.id, ready);
}

/// The notes of game `id` under their heading, taking input once `ready` (they have
/// arrived): saved half a second after typing stops; "Saved" then stays 1.6 s. Also the
/// notes of a Steam game's page.
pub(super) fn notes_field(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    id: GameId,
    ready: bool,
) {
    let shown_for = Duration::from_secs_f32(timing::SAVED_FOR);
    let saved = state
        .details
        .notes_saved
        .map(|t| env.mono.saturating_duration_since(t))
        .filter(|d| *d < shown_for);
    if let Some(d) = saved {
        ui.ctx().request_repaint_after(shown_for - d);
    }
    let heading = rows::heading_note(
        ui,
        s::NOTES(),
        saved.is_some().then_some(s::NOTES_SAVED()),
        true,
        false,
    );
    if saved.is_some() {
        crate::a11y::describe(&heading, s::NOTES_SAVED());
    }
    let r = ui
        .add_enabled_ui(ready, |ui| {
            TextField::new(&mut state.details.notes, "details.notes")
                .multiline()
                .hint(s::NOTES_HINT())
                .accessible_name(s::NOTES())
                .show(ui)
                .response
        })
        .inner;
    if r.changed() {
        state.details.notes_edited = Some(env.mono);
    }
    if let Some(t) = state.details.notes_edited {
        let due = Duration::from_secs_f32(timing::SAVED_AFTER);
        let since = env.mono.saturating_duration_since(t);
        if since >= due {
            state.details.notes_edited = None;
            state.details.notes_saved = Some(env.mono);
            ui.ctx().request_repaint_after(shown_for);
            crate::a11y::Announcer::announce(
                ui.ctx(),
                s::NOTES_SAVED(),
                crate::a11y::Politeness::Polite,
            );
            env.bridge.send(UiCommand::UpdateGame {
                id,
                edits: vec![GameEdit::Notes(state.details.notes.clone())],
            });
        } else {
            ui.ctx().request_repaint_after(due - since);
        }
    }
}

/// Show the Overview tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    crate::screens::debug::advanced_note(ui, state, env);
    how_it_runs(ui, state, env, g);
    where_it_lives(ui, state, env, g);
    notes(ui, state, env, g);
    what_it_is(ui, state, env, g);
}
