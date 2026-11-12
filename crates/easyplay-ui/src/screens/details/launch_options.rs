//! Launch options tab (SCREEN_SPECS_2 §11, UI-020): programs with roles and a run button,
//! arguments with a live split preview, working folder, stop grace, and environment
//! variables whose names are checked against EasyPlay's rules (reserved names name the
//! setting that controls them).

use easyplay_core::model::{EnvVar, ExeRole, GameSummary, WorkingDir};
use easyplay_core::umu::env::{EnvRule, classify};
use egui::{Align, Layout, Popup, Ui};

use super::card;
use super::permissions::{issue_line, section};
use crate::app::UiState;
use crate::bridge::{GameEdit, UiCommand};
use crate::input::picker::PickPurpose;
use crate::screens::Env;
use crate::strings;
use crate::strings::details as s;
use crate::theme::tokens::{icon as icon_size, space};
use crate::theme::{self, Type, icons};
use crate::vm::details::{LaunchDraft, ProgramEdit, ProgramField};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::{Button, Size};
use crate::widgets::context_menu::{danger_item, item};
use crate::widgets::icon_button::IconButton;
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::select;
use crate::widgets::text_field::TextField;

/// Stop grace choices in seconds.
pub const GRACE: [u8; 5] = [5, 10, 20, 30, 60];

/// The message for a variable name, if it cannot be saved.
pub fn env_problem(v: &EnvVar, developer: bool) -> Option<String> {
    if v.name.is_empty() {
        return None;
    }
    match classify(&v.name) {
        EnvRule::Allowed => None,
        EnvRule::Reserved => Some(s::reserved_var(&v.name)),
        EnvRule::Dangerous if developer => None,
        EnvRule::Dangerous => Some(s::DEV_ONLY_VAR.to_owned()),
        EnvRule::Invalid => Some(s::INVALID_VAR.to_owned()),
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
        ExeRole::Primary => s::ROLE_PRIMARY,
        ExeRole::Alternate => s::ROLE_ALTERNATE,
        ExeRole::Launcher => s::ROLE_LAUNCHER,
        ExeRole::Tool => s::ROLE_TOOL,
        ExeRole::Installer => s::ROLE_INSTALLER,
    }
}

/// The inline editor of a program's label or arguments: `Some(Some(edit))` saved,
/// `Some(None)` cancelled, `None` still editing.
fn program_editor(ui: &mut Ui, pe: &mut ProgramEdit) -> Option<Option<GameEdit>> {
    let (name, help) = match pe.field {
        ProgramField::Label => (s::LABEL_FIELD, s::LABEL_HELP),
        ProgramField::Args => (s::PROGRAM_ARGS_FIELD, s::PROGRAM_ARGS_HELP),
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
        issue_line(ui, s::ARGS_UNBALANCED, true);
    }
    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    ui.horizontal(|ui| {
        let save = ui.add_enabled(valid, Button::new(s::SAVE_PROGRAM).size(Size::Sm));
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
        let cancel = ui
            .add(Button::ghost(s::CANCEL_PROGRAM).size(Size::Sm))
            .clicked();
        if (cancel || escape) && out.is_none() {
            out = Some(None);
        }
    });
    out
}

/// Show the Launch options tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let id = g.id;
    let Some(d) = state.details.record().cloned() else {
        if state.details.detail.show_skeleton(env.mono) {
            super::skeleton_lines(ui, 6);
        }
        return;
    };
    ui.set_max_width(840.0);
    // Programs.
    let mut exes = d.game.executables.clone();
    exes.sort_by_key(|e| (e.role != ExeRole::Primary, e.role.as_str(), e.path.clone()));
    let mut edit = None;
    let mut open_edit: Option<ProgramEdit> = None;
    card(ui, s::PROGRAMS_TITLE, |ui| {
        for e in &exes {
            ui.horizontal(|ui| {
                let glyph = if e.role == ExeRole::Primary {
                    icons::PLAY
                } else {
                    icons::FILE_EXE
                };
                ui.label(icons::icon(glyph, icon_size::INLINE).color(p.text_secondary));
                let file = e
                    .path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                ui.label(Type::BodyStrong.rich(&file).color(p.text_primary));
                ui.label(
                    Type::Body
                        .rich(s::role_with_label(role_label(e.role), e.label.as_deref()))
                        .color(p.text_secondary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let more = ui.add(IconButton::new(icons::DOTS, &s::program_menu(&file)));
                    Popup::menu(&more).show(|ui| {
                        if e.role != ExeRole::Primary
                            && item(ui, Some(icons::PLAY), s::MAKE_MAIN, None, false).clicked()
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
                        if item(ui, None, s::RENAME_LABEL, None, false).clicked() {
                            open_edit = Some(ProgramEdit {
                                exe: e.id,
                                field: ProgramField::Label,
                                text: e.label.clone().unwrap_or_default(),
                            });
                        }
                        if item(ui, None, s::PROGRAM_ARGS, None, false).clicked() {
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
                            && danger_item(ui, Some(icons::TRASH), s::REMOVE_PROGRAM).clicked()
                        {
                            edit = Some(GameEdit::RemoveExe(e.id));
                        }
                    });
                    if ui
                        .add(IconButton::new(icons::PLAY, &s::run_program(&file)))
                        .clicked()
                    {
                        env.bridge.send(UiCommand::Launch {
                            id,
                            exe: Some(e.id),
                        });
                    }
                });
            });
            if e.args_override.is_some()
                && state
                    .details
                    .program_edit
                    .as_ref()
                    .is_none_or(|x| x.exe != e.id)
            {
                let args = e
                    .args_override
                    .as_deref()
                    .map(easyplay_core::model::join_args)
                    .unwrap_or_default();
                ui.label(Type::Caption.rich(s::own_args(&args)).color(p.text_muted));
            }
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
        ui.horizontal(|ui| {
            if ui
                .add(Button::new(s::ADD_PROGRAM).icon(icons::PLUS).size(Size::Sm))
                .clicked()
            {
                state.picks.push(PickPurpose::AddProgram(id));
            }
            if ui
                .add(Button::ghost(strings::menu::RUN_OTHER).size(Size::Sm))
                .clicked()
            {
                state.picks.push(PickPurpose::RunProgram(id));
            }
        });
    });
    if let Some(pe) = open_edit {
        state.details.program_edit = Some(pe);
    }
    if let Some(e) = edit {
        env.bridge
            .send(UiCommand::UpdateGame { id, edits: vec![e] });
        state.toast(strings::feedback::SAVED, ToastKind::Success, None, env.mono);
    }
    let developer = env.bridge.prefs().advanced.developer_mode;
    let Some(l) = state.details.launch.as_mut() else {
        return;
    };
    // Arguments.
    section(ui, s::ARGUMENTS, None);
    let split = easyplay_core::model::split_args(&l.args_text);
    TextField::new(&mut l.args_text, "launch.args")
        .accessible_name(s::ARGUMENTS)
        .mono()
        .help(s::ARGUMENTS_HELP)
        .show(ui);
    match &split {
        Ok(a) if !l.args_text.trim().is_empty() => {
            let r = ui.label(
                Type::Caption
                    .rich(s::args_preview(a.len()))
                    .color(p.text_muted),
            );
            crate::a11y::labelled(
                ui.ctx(),
                r.id,
                egui::Role::Status,
                &s::args_preview(a.len()),
            );
        }
        Err(_) => issue_line(ui, s::ARGS_UNBALANCED, true),
        _ => {}
    }
    // Working folder.
    section(ui, s::START_IN, None);
    let mut wd = match l.config.working_dir {
        WorkingDir::ExeDir => 0,
        WorkingDir::GameRoot => 1,
        WorkingDir::Custom(_) => 2,
    };
    let opts: Vec<Segment<'_>> = [s::WD_PROGRAM, s::WD_GAME, s::WD_OTHER]
        .iter()
        .map(|x| Segment {
            icon: None,
            label: x,
        })
        .collect();
    if segmented(ui, s::START_IN, &mut wd, &opts, false).changed() {
        match wd {
            0 => l.config.working_dir = WorkingDir::ExeDir,
            1 => l.config.working_dir = WorkingDir::GameRoot,
            _ => state.picks.push(PickPurpose::WorkingDir(id)),
        }
    }
    let Some(l) = state.details.launch.as_mut() else {
        return;
    };
    if let WorkingDir::Custom(p2) = &l.config.working_dir {
        ui.label(
            Type::Mono
                .rich(p2.display().to_string())
                .color(p.text_secondary),
        );
    }
    // Stop grace.
    section(ui, s::WHEN_STOPPING, None);
    ui.horizontal(|ui| {
        let mut gi = GRACE
            .iter()
            .position(|x| *x == l.config.stop_grace_secs)
            .unwrap_or(1);
        let labels: Vec<String> = GRACE.iter().map(|x| s::seconds(*x)).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        if select(ui, s::WHEN_STOPPING, None, None, &mut gi, &refs, false).changed() {
            l.config.stop_grace_secs = GRACE[gi];
        }
        ui.label(Type::Body.rich(s::BEFORE_FORCING).color(p.text_secondary));
    });
    // Advanced: environment.
    ui.add_space(space::S4);
    let adv = state.details.launch_advanced;
    let n = l.config.env.len();
    if ui
        .add(Button::ghost(&s::advanced(n)).icon(if adv {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        }))
        .clicked()
    {
        state.details.launch_advanced = !adv;
    }
    if !state.details.launch_advanced {
        return;
    }
    let Some(l) = state.details.launch.as_mut() else {
        return;
    };
    ui.label(Type::BodyStrong.rich(s::ENV_VARS).color(p.text_primary));
    if l.config.env.is_empty() {
        ui.label(Type::Body.rich(s::NO_VARS).color(p.text_secondary));
    }
    let mut remove = None;
    let count = l.config.env.len();
    for i in 0..count {
        let name_salt = format!("env.name.{i}");
        let value_salt = format!("env.value.{i}");
        let name_label = s::var_name(i + 1);
        let value_label = s::var_value(i + 1);
        let problem = env_problem(&l.config.env[i], developer);
        ui.horizontal(|ui| {
            let v = &mut l.config.env[i];
            TextField::new(&mut v.name, &name_salt)
                .accessible_name(&name_label)
                .mono()
                .width(200.0)
                .show(ui);
            ui.label(Type::Mono.rich("=").color(p.text_muted));
            TextField::new(&mut v.value, &value_salt)
                .accessible_name(&value_label)
                .mono()
                .width(260.0)
                .show(ui);
            if ui
                .add(IconButton::new(icons::CLOSE, &s::remove_var(i + 1)))
                .clicked()
            {
                remove = Some(i);
            }
        });
        if let Some(pr) = problem {
            issue_line(ui, &pr, true);
        }
    }
    if let Some(i) = remove {
        l.config.env.remove(i);
    }
    if ui
        .add(Button::new(s::ADD_VAR).icon(icons::PLUS).size(Size::Sm))
        .clicked()
    {
        l.config.env.push(EnvVar {
            name: String::new(),
            value: String::new(),
        });
    }
}
