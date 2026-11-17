//! Launch options tab (easyplay-redesign.html `tabLaunch`; SCREEN_SPECS_2 §11, UI-020):
//! setting rows in a 620 column. *Programs*: each program's file name in mono with its role
//! under it, a run button and a menu (make main, mark as, rename its label, its own
//! arguments, remove); then *Add program…* and *Run another program once…*. *Arguments*: a
//! mono field with what it comes to under it. *Starting and stopping*: the working folder
//! and how long a stop waits. *Environment variables*: a NAME = value row that adds one, and
//! the variables set, each editable; a name is checked against EasyPlay's rules (reserved
//! names name the setting that controls them) and suggests Proton's options as it is typed,
//! each with what it does (ADR-0027).

use easyplay_core::model::{EnvVar, ExeRole, GameSummary, WorkingDir};
use easyplay_core::umu::env::{EnvRule, classify};
use easyplay_core::umu::flags::{self, Builds, Flag};
use egui::{
    Align, CornerRadius, Key, Layout, Modifiers, Popup, Rect, Response, Sense, Ui, UiBuilder, pos2,
    vec2,
};

use super::column;
use super::permissions::issue_line;
use crate::app::UiState;
use crate::bridge::{GameEdit, UiCommand};
use crate::input::picker::PickPurpose;
use crate::screens::Env;
use crate::strings;
use crate::strings::details as s;
use crate::theme::tokens::{control, game as gm, layout, radius, space};
use crate::theme::{self, Type, icons};
use crate::vm::details::{LaunchDraft, ProgramEdit, ProgramField};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::Button;
use crate::widgets::context_menu::{danger_item, item};
use crate::widgets::icon_button::IconButton;
use crate::widgets::rows::{self, SettingRow};
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::select;
use crate::widgets::text_field::TextField;

/// Stop grace choices in seconds.
pub const GRACE: [u8; 5] = [5, 10, 20, 30, 60];

/// Proton options offered at once under a name field.
const SUGGESTIONS: usize = 6;

/// Whether this game's compatibility layer (by its name) may read `f`: every build reads
/// the common options; the others are read by GE-Proton and CachyOS builds, and a layer
/// whose name says neither (*Default* not yet resolved, a custom folder) gets the benefit of
/// the doubt.
fn layer_reads(f: &Flag, layer: &str) -> bool {
    let layer = layer.to_ascii_lowercase();
    let valve = layer.contains("proton") && !layer.contains("ge-proton");
    f.builds == Builds::Every || layer.contains("cachyos") || !valve
}

/// One suggestion: the name, and beneath it what the option does.
fn suggestion(ui: &mut Ui, f: &Flag, lit: bool) -> Response {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width();
    let pad = space::S3 - space::S1;
    let name = Type::Mono.galley(ui, f.name, p.t1, w - pad * 2.0, 1);
    let what = Type::Caption.galley(ui, f.summary, p.t3, w - pad * 2.0, usize::MAX);
    let h = space::S1 * 2.0 + name.size().y + what.size().y;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h.max(control::MENU_ITEM)), Sense::click());
    let label = format!("{}: {}", f.name, f.summary);
    crate::a11y::option(&resp, egui::accesskit::Role::ListBoxOption, &label, lit);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if lit || resp.hovered() {
            painter.rect_filled(rect, CornerRadius::same(radius::SM), p.o2);
        }
        let x = rect.left() + pad;
        let top = rect.top() + space::S1;
        let nh = name.size().y;
        painter.galley(pos2(x, top), name, p.t1);
        painter.galley(pos2(x, top + nh), what, p.t3);
    }
    resp
}

/// The Proton options matching a variable's name, under its field (ADR-0027). The arrow
/// keys move through them and Enter takes one, as does a click; the option's usual value
/// fills an empty value. Returns whether one was taken.
fn suggestions(ui: &mut Ui, field: &Response, v: &mut EnvVar, taken: &[&str]) -> bool {
    let ctx = ui.ctx().clone();
    let at_id = field.id.with("suggestion");
    let hover_id = field.id.with("suggestions.hovered");
    let hovered = ctx.data(|d| d.get_temp::<bool>(hover_id)).unwrap_or(false);
    let list = flags::suggest(&v.name, taken, SUGGESTIONS);
    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    let open = (field.has_focus() || hovered || enter) && !list.is_empty();
    if !open {
        ctx.data_mut(|d| {
            d.remove_temp::<usize>(at_id);
            d.remove_temp::<bool>(hover_id);
        });
        return false;
    }
    let last = list.len() - 1;
    let mut at = ctx
        .data(|d| d.get_temp::<usize>(at_id))
        .unwrap_or(0)
        .min(last);
    if field.has_focus() {
        ui.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                at = (at + 1).min(last);
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                at = at.saturating_sub(1);
            }
        });
    }
    if field.changed() {
        at = 0;
    }
    let mut picked = enter.then_some(at);
    let w = layout::MENU_W[1];
    let shown = Popup::from_response(field)
        .open(true)
        .accessible_name(s::PROTON_OPTIONS())
        .width(w)
        .frame(egui::Frame::NONE)
        .show(|ui| {
            crate::widgets::context_menu::frame(ui, |ui| {
                ui.set_width(w);
                for (i, f) in list.iter().enumerate() {
                    if suggestion(ui, f, i == at).clicked() {
                        picked = Some(i);
                    }
                }
            })
        });
    let over = shown.is_some_and(|r| r.response.contains_pointer());
    ctx.data_mut(|d| {
        d.insert_temp(at_id, at);
        d.insert_temp(hover_id, over);
    });
    let Some(f) = picked.and_then(|i| list.get(i)) else {
        return false;
    };
    v.name = f.name.to_owned();
    if v.value.is_empty() {
        v.value = f.value.to_owned();
    }
    ctx.data_mut(|d| {
        d.remove_temp::<usize>(at_id);
        d.remove_temp::<bool>(hover_id);
    });
    true
}

/// The message for a variable name, if it cannot be saved.
pub fn env_problem(v: &EnvVar, developer: bool) -> Option<String> {
    if v.name.is_empty() {
        return None;
    }
    match classify(&v.name) {
        EnvRule::Allowed => None,
        EnvRule::Reserved => Some(s::reserved_var(&v.name)),
        EnvRule::Dangerous if developer => None,
        EnvRule::Dangerous => Some(s::DEV_ONLY_VAR().to_owned()),
        EnvRule::Invalid => Some(s::INVALID_VAR().to_owned()),
    }
}

/// Variables that cannot be saved (always checked as if Developer Mode were off for
/// reserved and invalid names).
pub fn env_errors(l: &LaunchDraft) -> usize {
    l.config
        .env
        .iter()
        .filter(|v| {
            matches!(classify(&v.name), EnvRule::Reserved | EnvRule::Invalid) && !v.name.is_empty()
        })
        .count()
}

fn role_label(r: ExeRole) -> &'static str {
    match r {
        ExeRole::Primary => s::ROLE_PRIMARY(),
        ExeRole::Alternate => s::ROLE_ALTERNATE(),
        ExeRole::Launcher => s::ROLE_LAUNCHER(),
        ExeRole::Tool => s::ROLE_TOOL(),
        ExeRole::Installer => s::ROLE_INSTALLER(),
    }
}

/// The inline editor of a program's label or arguments: `Some(Some(edit))` saved,
/// `Some(None)` cancelled, `None` still editing.
fn program_editor(ui: &mut Ui, pe: &mut ProgramEdit) -> Option<Option<GameEdit>> {
    let (name, help) = match pe.field {
        ProgramField::Label => (s::LABEL_FIELD(), s::LABEL_HELP()),
        ProgramField::Args => (s::PROGRAM_ARGS_FIELD(), s::PROGRAM_ARGS_HELP()),
    };
    let mut out = None;
    let field = TextField::new(&mut pe.text, "launch.program_edit")
        .accessible_name(name)
        .help(help);
    let field = if pe.field == ProgramField::Args {
        field.mono()
    } else {
        field
    };
    let r = field.show(ui).response;
    let split = easyplay_core::model::split_args(&pe.text);
    let valid = match pe.field {
        ProgramField::Label => {
            pe.text.trim().chars().count() <= easyplay_core::model::EXE_LABEL_MAX_CHARS
        }
        ProgramField::Args => split.is_ok(),
    };
    if pe.field == ProgramField::Args && split.is_err() {
        issue_line(ui, s::ARGS_UNBALANCED(), true);
    }
    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    ui.add_space(gm::FINE_TOP);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        let save = ui.add_enabled(valid, Button::new(s::SAVE_PROGRAM()));
        if (save.clicked() || (enter && valid)) && out.is_none() {
            let text = pe.text.trim().to_owned();
            out = Some(Some(match pe.field {
                ProgramField::Label => {
                    GameEdit::ExeLabel(pe.exe, (!text.is_empty()).then_some(text))
                }
                ProgramField::Args => GameEdit::ExeArgs(
                    pe.exe,
                    (!text.is_empty()).then(|| split.clone().unwrap_or_default()),
                ),
            }));
        }
        let cancel = ui.add(Button::ghost(s::CANCEL_PROGRAM())).clicked();
        if (cancel || escape) && out.is_none() {
            out = Some(None);
        }
    });
    out
}

/// A row of buttons whose text keeps the column's axis (their plates hang 10 to the left).
fn hanging<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let at = ui.cursor().min;
    let w = ui.available_width();
    let mut line = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                pos2(at.x - gm::ROW_BUTTONS[1], at.y),
                vec2(w + gm::ROW_BUTTONS[1], f32::INFINITY),
            ))
            .layout(Layout::left_to_right(Align::Min)),
    );
    let out = add(&mut line);
    ui.allocate_exact_size(vec2(w, line.min_rect().height()), Sense::hover());
    out
}

/// *Programs*.
fn programs(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let Some(d) = state.details.record().cloned() else {
        return;
    };
    let id = g.id;
    rows::heading(ui, s::PROGRAMS_TITLE(), true);
    let mut exes = d.game.executables.clone();
    exes.sort_by_key(|e| (e.role != ExeRole::Primary, e.role.as_str(), e.path.clone()));
    let mut edit = None;
    let mut open_edit: Option<ProgramEdit> = None;
    for e in &exes {
        let file = e
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let mut help = s::shown_as(role_label(e.role), e.label.as_deref());
        let editing = state
            .details
            .program_edit
            .as_ref()
            .is_some_and(|x| x.exe == e.id);
        if let Some(args) = e.args_override.as_deref().filter(|_| !editing) {
            help = format!(
                "{help}\n{}",
                s::own_args(&easyplay_core::model::join_args(args))
            );
        }
        SettingRow::new(&file)
            .mono()
            .help(&help)
            .id_salt(("program", e.id))
            .show(ui, |ui| {
                let run_name = s::run_program(&file);
                let run = ui.add(IconButton::new(icons::PLAY, &run_name).filled(true));
                // What the row says under the file name: its role and its own arguments.
                crate::a11y::describe(&run, &help);
                if run.clicked() {
                    env.bridge.send(UiCommand::Launch {
                        id,
                        exe: Some(e.id),
                    });
                }
                let more = ui.add(IconButton::new(icons::DOTS, &s::program_menu(&file)));
                Popup::menu(&more).show(|ui| {
                    if e.role != ExeRole::Primary
                        && item(ui, None, s::MAKE_MAIN(), None, false).clicked()
                    {
                        edit = Some(GameEdit::ExeRole(e.id, ExeRole::Primary));
                    }
                    for r in [ExeRole::Alternate, ExeRole::Launcher, ExeRole::Tool] {
                        if e.role != r
                            && e.role != ExeRole::Primary
                            && item(ui, None, &s::mark_as(role_label(r)), None, false).clicked()
                        {
                            edit = Some(GameEdit::ExeRole(e.id, r));
                        }
                    }
                    if item(ui, None, s::RENAME_LABEL(), None, false).clicked() {
                        open_edit = Some(ProgramEdit {
                            exe: e.id,
                            field: ProgramField::Label,
                            text: e.label.clone().unwrap_or_default(),
                        });
                    }
                    if item(ui, None, s::PROGRAM_ARGS(), None, false).clicked() {
                        open_edit = Some(ProgramEdit {
                            exe: e.id,
                            field: ProgramField::Args,
                            text: e
                                .args_override
                                .as_deref()
                                .map(easyplay_core::model::join_args)
                                .unwrap_or_default(),
                        });
                    }
                    if e.role != ExeRole::Primary
                        && danger_item(ui, None, s::REMOVE_PROGRAM()).clicked()
                    {
                        edit = Some(GameEdit::RemoveExe(e.id));
                    }
                });
            });
        if let Some(pe) = state.details.program_edit.as_mut()
            && pe.exe == e.id
            && let Some(done) = program_editor(ui, pe)
        {
            if let Some(x) = done {
                edit = Some(x);
            }
            state.details.program_edit = None;
        }
    }
    ui.add_space(gm::ROW_BUTTONS[0]);
    let (add, once) = hanging(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let add = ui
            .add(Button::ghost(s::ADD_PROGRAM()).icon(icons::PLUS))
            .clicked();
        let once = ui.add(Button::ghost(s::RUN_ONCE())).clicked();
        (add, once)
    });
    if add {
        state.picks.push(PickPurpose::AddProgram(id));
    }
    if once {
        state.picks.push(PickPurpose::RunProgram(id));
    }
    if let Some(pe) = open_edit {
        state.details.program_edit = Some(pe);
    }
    if let Some(e) = edit {
        env.bridge
            .send(UiCommand::UpdateGame { id, edits: vec![e] });
        state.toast(
            strings::feedback::SAVED(),
            ToastKind::Success,
            None,
            env.mono,
        );
    }
}

/// The widths of an environment row's name and value fields, given the button at its end.
fn env_widths(w: f32, end: f32, equals: f32) -> (f32, f32) {
    let gaps = space::S2 * 3.0;
    let room = (w - end - equals - gaps).max(space::S16);
    let name = room / (1.0 + gm::ENV_VALUE_SHARE);
    (name, room - name)
}

/// *Environment variables*: the row that adds one, then those set.
fn variables(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary, layer: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let developer = env.bridge.prefs().advanced.developer_mode;
    let Some(l) = state.details.launch.as_mut() else {
        return;
    };
    let set = l.config.env.iter().filter(|v| !v.name.is_empty()).count();
    let note = (set > 0).then(|| s::vars_set(set));
    rows::heading_note(ui, s::ENV_VARS(), note.as_deref(), false, false);
    let w = ui.available_width();
    let equals = Type::Mono.line(ui, "=", p.t3).size().x;
    let taken: Vec<String> = l.config.env.iter().map(|v| v.name.clone()).collect();
    let taken: Vec<&str> = taken.iter().map(String::as_str).collect();

    // The row that adds a variable.
    let mut fresh = EnvVar {
        name: std::mem::take(&mut state.details.new_var_name),
        value: std::mem::take(&mut state.details.new_var_value),
    };
    let add_w = Button::new(s::ADD()).width(ui);
    let (nw, vw) = env_widths(w, add_w, equals);
    let mut added = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S2;
        let name = TextField::new(&mut fresh.name, "env.new.name")
            .accessible_name(s::NEW_VAR_NAME())
            .hint(s::NAME_HINT())
            .mono()
            .width(nw)
            .show(ui)
            .response;
        ui.label(Type::Mono.rich("=").color(p.t3));
        let value = TextField::new(&mut fresh.value, "env.new.value")
            .accessible_name(s::NEW_VAR_VALUE())
            .hint(s::VALUE_HINT())
            .mono()
            .width(vw)
            .show(ui)
            .response;
        if suggestions(ui, &name, &mut fresh, &taken) {
            value.request_focus();
        }
        let enter = value.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if (ui.add(Button::new(s::ADD())).clicked() || enter) && !fresh.name.trim().is_empty() {
            added = true;
        }
    });
    if let Some(pr) = env_problem(&fresh, developer) {
        issue_line(ui, &pr, true);
    }
    if added && env_problem(&fresh, developer).is_none() {
        l.config.env.push(EnvVar {
            name: fresh.name.trim().to_owned(),
            value: fresh.value,
        });
    } else {
        state.details.new_var_name = fresh.name;
        state.details.new_var_value = fresh.value;
    }
    let Some(l) = state.details.launch.as_mut() else {
        return;
    };

    // The variables set.
    let mut remove = None;
    let count = l.config.env.len();
    let (nw, vw) = env_widths(w, control::BUTTON, equals);
    for i in 0..count {
        let name_salt = format!("env.name.{i}");
        let value_salt = format!("env.value.{i}");
        let name_label = s::var_name(i + 1);
        let value_label = s::var_value(i + 1);
        let taken: Vec<String> = l.config.env.iter().map(|v| v.name.clone()).collect();
        let taken: Vec<&str> = taken.iter().map(String::as_str).collect();
        ui.add_space(space::S2);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            let v = &mut l.config.env[i];
            let name = TextField::new(&mut v.name, &name_salt)
                .accessible_name(&name_label)
                .hint(s::FIND_PROTON_OPTION())
                .mono()
                .width(nw)
                .show(ui)
                .response;
            ui.label(Type::Mono.rich("=").color(p.t3));
            let value = TextField::new(&mut v.value, &value_salt)
                .accessible_name(&value_label)
                .mono()
                .width(vw)
                .show(ui)
                .response;
            if suggestions(ui, &name, v, &taken) {
                value.request_focus();
            }
            if ui
                .add(IconButton::new(icons::CLOSE, &s::remove_var(i + 1)))
                .clicked()
            {
                remove = Some(i);
            }
        });
        let v = &l.config.env[i];
        if let Some(pr) = env_problem(v, developer) {
            super::fine(ui, &pr, p.coral);
        } else if let Some(f) = flags::find(&v.name) {
            // What a known option does, said once, under its row.
            if layer_reads(f, layer) {
                super::fine(ui, f.summary, p.t3);
            } else {
                super::fine(ui, &s::not_read_by(f.summary, layer), p.amber);
            }
        }
    }
    if let Some(i) = remove {
        l.config.env.remove(i);
    }
    let _ = g;
}

/// Show the Launch options tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    column(ui, |ui| body(ui, state, env, g));
}

fn body(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let id = g.id;
    let Some(d) = state.details.record().cloned() else {
        if state.details.detail.show_skeleton(env.mono) {
            super::skeleton_lines(ui, 6);
        }
        return;
    };
    programs(ui, state, env, g);
    let Some(l) = state.details.launch.as_mut() else {
        return;
    };

    // Arguments.
    rows::heading(ui, s::ARGUMENTS(), false);
    let split = easyplay_core::model::split_args(&l.args_text);
    TextField::new(&mut l.args_text, "launch.args")
        .accessible_name(s::ARGUMENTS())
        .hint(s::ARGS_NONE())
        .mono()
        .wrapping()
        .show(ui);
    ui.add_space(gm::FINE_TOP);
    let note = match &split {
        Ok(a) => super::fine(ui, &s::args_note(a.len()), p.t3),
        Err(_) => super::fine(ui, s::ARGS_UNBALANCED(), p.coral),
    };
    let said = match &split {
        Ok(a) => s::args_note(a.len()),
        Err(_) => s::ARGS_UNBALANCED().to_owned(),
    };
    crate::a11y::labelled(ui.ctx(), note.id, egui::Role::Status, &said);

    // Starting and stopping.
    rows::heading(ui, s::STARTING_AND_STOPPING(), false);
    let mut wd = match l.config.working_dir {
        WorkingDir::ExeDir => 0,
        WorkingDir::GameRoot => 1,
        WorkingDir::Custom(_) => 2,
    };
    let custom = match &l.config.working_dir {
        WorkingDir::Custom(path) => path.display().to_string(),
        _ => String::new(),
    };
    let opts: Vec<Segment<'_>> = [s::WD_PROGRAM, s::WD_GAME, s::WD_OTHER]
        .iter()
        .map(|x| Segment {
            icon: None,
            label: x(),
        })
        .collect();
    let moved = SettingRow::new(s::START_IN()).help(&custom).show(ui, |ui| {
        segmented(ui, s::START_IN(), &mut wd, &opts, false).changed()
    });
    if moved {
        match wd {
            0 => l.config.working_dir = WorkingDir::ExeDir,
            1 => l.config.working_dir = WorkingDir::GameRoot,
            _ => state.picks.push(PickPurpose::WorkingDir(id)),
        }
    }
    let Some(l) = state.details.launch.as_mut() else {
        return;
    };
    SettingRow::new(s::WHEN_STOPPING())
        .help(s::STOP_WAIT_HELP())
        .show(ui, |ui| {
            let mut gi = GRACE
                .iter()
                .position(|x| *x == l.config.stop_grace_secs)
                .unwrap_or(1);
            let labels: Vec<String> = GRACE.iter().map(|x| s::seconds(*x)).collect();
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            if select(ui, s::WHEN_STOPPING(), None, None, &mut gi, &refs, false).changed() {
                l.config.stop_grace_secs = GRACE[gi.min(GRACE.len() - 1)];
            }
        });

    // Environment variables.
    let layer = d
        .runtime_resolved
        .clone()
        .unwrap_or_else(|| g.runtime_label.to_string());
    variables(ui, state, env, g, &layer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn community_options_are_doubted_only_on_a_build_that_names_itself() {
        let fsr4 = flags::find("PROTON_FSR4_UPGRADE").unwrap();
        let cap = flags::find("DXVK_FRAME_RATE").unwrap();
        assert!(!layer_reads(fsr4, "UMU-Proton 10.0-4"));
        assert!(!layer_reads(fsr4, "Proton 9.0"));
        assert!(layer_reads(fsr4, "GE-Proton10-25"));
        assert!(layer_reads(fsr4, "proton-cachyos-11.0-slr"));
        assert!(layer_reads(fsr4, "Default"));
        assert!(layer_reads(cap, "UMU-Proton 10.0-4"));
    }

    #[test]
    fn an_environment_row_gives_the_value_more_room() {
        let (n, v) = env_widths(620.0, 50.0, 8.0);
        assert!((v / n - gm::ENV_VALUE_SHARE).abs() < 0.01);
        assert!((n + v + 50.0 + 8.0 + space::S2 * 3.0 - 620.0).abs() < 0.01);
    }
}
