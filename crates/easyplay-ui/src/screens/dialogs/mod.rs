//! Dialogs (SCREEN_SPECS_2 §16, UI-027): the standard confirmations of §16.1, one dialog per
//! prompt kind of LOGIC_SPECS §4, and the generic error dialog built from a `UserError` with
//! a queue ("1 more problem"). Every default button is the safe choice; `Enter` activates it,
//! `Esc` cancels.

pub mod artwork;
pub mod shortcuts;
pub mod tags;

use easyplay_core::error::{RecoveryAction, UserError};
use easyplay_core::model::{PromptAnswer, PromptKind, PromptRequest};
use egui::{Context, Id, Ui};

use super::Env;
use super::actions;
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::route::{DialogId, Overlay};
use crate::strings;
use crate::strings::dialogs as d;
use crate::theme::tokens::space;
use crate::theme::{self, Icon, Type, icons};
use crate::vm::dialogs::{Confirm, Dialog};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::Variant;
use crate::widgets::modal::{Action, DialogOutcome, rich_dialog};
use crate::widgets::toggle::toggle;

fn secondary(label: &str) -> Action<'_> {
    Action {
        label,
        variant: Variant::Secondary,
    }
}

fn primary(label: &str) -> Action<'_> {
    Action {
        label,
        variant: Variant::Primary,
    }
}

fn danger(label: &str) -> Action<'_> {
    Action {
        label,
        variant: Variant::Danger,
    }
}

fn text(ui: &mut Ui, s: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.label(Type::Body.rich(s).color(p.text_secondary));
}

fn bullet(ui: &mut Ui, icon: Icon, s: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.horizontal(|ui| {
        ui.label(icons::icon(icon, theme::tokens::icon::INLINE).color(p.text_muted));
        ui.label(Type::Body.rich(s).color(p.text_primary));
    });
}

fn game_name(env: &Env<'_>, g: Option<easyplay_core::ids::GameId>) -> String {
    g.and_then(|g| env.bridge.library().get(g).map(|s| s.name.to_string()))
        .unwrap_or_default()
}

/// Show the dialog `dialog` of the queue.
pub fn show(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, dialog: DialogId) {
    let Some(dlg) = state.dialogs.get(dialog).cloned() else {
        state.nav.close(Overlay::Dialog(dialog));
        state.sync_dialog();
        return;
    };
    let done = match dlg {
        Dialog::Confirm(c) => confirm(ctx, id, state, env, dialog, c),
        Dialog::Prompt(p) => prompt(ctx, id, state, env, &p),
        Dialog::Error(e) => error(ctx, id, state, env, &e),
    };
    if done {
        state.nav.close(Overlay::Dialog(dialog));
        state.dialogs.close(dialog);
        state.sync_dialog();
    }
}

fn confirm(
    ctx: &Context,
    id: Id,
    state: &mut UiState,
    env: &mut Env<'_>,
    dialog: DialogId,
    c: Confirm,
) -> bool {
    let lib = env.bridge.library().clone();
    match c {
        Confirm::RemoveGames { ids, mut options } => {
            let title = if ids.len() == 1 {
                d::remove_title(&game_name(env, ids.first().copied()))
            } else {
                d::remove_many_title(ids.len())
            };
            let o = rich_dialog(
                ctx,
                id,
                Some(icons::TRASH),
                &title,
                &[secondary(d::CANCEL), danger(d::REMOVE)],
                0,
                |ui| {
                    text(ui, d::REMOVE_BODY);
                    ui.add_space(space::S1);
                    toggle(
                        ui,
                        &mut options.delete_private_storage,
                        d::REMOVE_STORAGE,
                        Some(d::REMOVE_STORAGE_HINT),
                    );
                    if options.delete_private_storage {
                        toggle(ui, &mut options.keep_save_data, d::KEEP_SAVES, None);
                    }
                    toggle(ui, &mut options.delete_shortcuts, d::REMOVE_SHORTCUTS, None);
                    toggle(ui, &mut options.keep_history, d::KEEP_HISTORY, None);
                },
            );
            if let Some(Dialog::Confirm(Confirm::RemoveGames { options: o2, .. })) =
                state.dialogs.get_mut(dialog)
            {
                *o2 = options;
            }
            if o == DialogOutcome::Chose(1) {
                for g in &ids {
                    env.bridge.send(UiCommand::RemoveGame { id: *g, options });
                }
                state.library.selection.clear();
            }
            o != DialogOutcome::Open
        }
        Confirm::ResetPrefix {
            id: game,
            mut options,
        } => {
            let name = game_name(env, Some(game));
            let o = rich_dialog(
                ctx,
                id,
                Some(icons::REFRESH),
                &d::reset_title(&name),
                &[secondary(d::CANCEL), danger(d::RESET)],
                0,
                |ui| {
                    text(ui, d::RESET_DELETED);
                    bullet(ui, icons::TRASH, d::RESET_DELETED_1);
                    bullet(ui, icons::TRASH, d::RESET_DELETED_2);
                    text(ui, d::RESET_KEPT);
                    bullet(ui, icons::CHECK, d::RESET_KEPT_1);
                    toggle(ui, &mut options.keep_save_data, d::KEEP_SAVES, None);
                },
            );
            if let Some(Dialog::Confirm(Confirm::ResetPrefix { options: o2, .. })) =
                state.dialogs.get_mut(dialog)
            {
                *o2 = options;
            }
            if o == DialogOutcome::Chose(1) {
                env.bridge
                    .send(UiCommand::ResetPrefix { id: game, options });
            }
            o != DialogOutcome::Open
        }
        Confirm::DiscardChanges(after) => {
            let o = crate::widgets::modal::dialog(
                ctx,
                id,
                None,
                d::DISCARD_TITLE,
                d::DISCARD_BODY,
                &[primary(d::KEEP_EDITING), secondary(d::DISCARD)],
                0,
            );
            if o == DialogOutcome::Chose(1) {
                state.details.discard();
                state.apply_nav(after);
            }
            o != DialogOutcome::Open
        }
        Confirm::WidenProtection { ids, preset } => {
            let title = if ids.len() == 1 {
                d::widen_title(&game_name(env, ids.first().copied()), preset)
            } else {
                d::widen_many_title(ids.len(), preset)
            };
            let running = ids
                .iter()
                .filter(|g| {
                    lib.get(**g)
                        .is_some_and(|s| s.activity != easyplay_core::model::Activity::Idle)
                })
                .count();
            let o = rich_dialog(
                ctx,
                id,
                Some(icons::GLOBE),
                &title,
                &[secondary(d::CANCEL), primary(d::ALLOW)],
                0,
                |ui| {
                    text(ui, d::WIDEN_BODY);
                    for s in d::widen_statements(preset) {
                        bullet(ui, icons::PLUS, s);
                    }
                    if running > 0 {
                        text(ui, &d::skipped_running(running));
                    }
                },
            );
            if o == DialogOutcome::Chose(1) {
                env.bridge.send(UiCommand::ApplyPreset { ids, preset });
            }
            o != DialogOutcome::Open
        }
        Confirm::RemoveLayer {
            name,
            label,
            size,
            used_by,
        } => {
            let o = rich_dialog(
                ctx,
                id,
                Some(icons::LAYERS),
                &d::remove_layer_title(&label),
                &[secondary(d::CANCEL), danger(d::REMOVE_LAYER)],
                0,
                |ui| {
                    if let Some(s) = size {
                        text(ui, &d::frees(s));
                    }
                    text(ui, &d::layer_users(used_by));
                },
            );
            if o == DialogOutcome::Chose(1) {
                env.bridge.send(UiCommand::RemoveRuntime { name });
            }
            o != DialogOutcome::Open
        }
        Confirm::ClearStatistics | Confirm::ResetGameStatistics(_) => {
            let game = match c {
                Confirm::ResetGameStatistics(g) => Some(g),
                _ => None,
            };
            let (sessions, ms) = match game {
                Some(g) => lib.get(g).map_or((0, 0), |s| {
                    (u64::from(s.session_count), s.total_playtime_ms)
                }),
                None => lib.games.iter().fold((0, 0), |(n, t), g| {
                    (n + u64::from(g.session_count), t + g.total_playtime_ms)
                }),
            };
            let title = match game {
                Some(g) => d::clear_game_title(&game_name(env, Some(g))),
                None => d::CLEAR_TITLE.to_owned(),
            };
            let o = rich_dialog(
                ctx,
                id,
                Some(icons::CHART),
                &title,
                &[secondary(d::CANCEL), danger(d::CLEAR)],
                0,
                |ui| text(ui, &d::clear_body(sessions, ms)),
            );
            if o == DialogOutcome::Chose(1) {
                env.bridge.send(match game {
                    Some(g) => UiCommand::ResetGameStatistics { id: g },
                    None => UiCommand::ClearStatistics,
                });
            }
            o != DialogOutcome::Open
        }
        Confirm::DeveloperMode => {
            let o = rich_dialog(
                ctx,
                id,
                Some(icons::TERMINAL),
                d::DEV_TITLE,
                &[secondary(d::CANCEL), primary(d::TURN_ON)],
                0,
                |ui| {
                    text(ui, d::DEV_BODY);
                    text(ui, d::DEV_UNSAFE);
                },
            );
            if o == DialogOutcome::Chose(1) {
                super::settings::set_bool(state, env, "advanced.developer_mode", true, |p, v| {
                    p.advanced.developer_mode = v;
                });
            }
            o != DialogOutcome::Open
        }
    }
}

/// Title, icon, body lines and (label, emphasis, answer) actions of a prompt dialog.
type PromptView<'a> = (
    String,
    Icon,
    Vec<String>,
    Vec<(&'a str, Variant, PromptAnswer)>,
);

fn answer(env: &mut Env<'_>, p: &PromptRequest, a: PromptAnswer) {
    debug_assert!(p.kind.accepts(&a));
    env.bridge.send(UiCommand::AnswerPrompt {
        prompt: p.id,
        answer: a,
    });
}

fn prompt(
    ctx: &Context,
    id: Id,
    state: &mut UiState,
    env: &mut Env<'_>,
    p: &PromptRequest,
) -> bool {
    let name = game_name(env, p.kind.game());
    let mut remember = state.dialogs.form.remember;
    let mut choice = state.dialogs.form.choice;
    // (title, body lines, actions with answers, default index, icon)
    let (title, icon, body, acts): PromptView<'_> = match &p.kind {
        PromptKind::ExeChanged { old, new, .. } => (
            d::exe_changed_title(&name),
            icons::FILE_EXE,
            vec![
                d::exe_changed_sizes(old.size, new.size),
                d::EXE_CHANGED_BODY.to_owned(),
            ],
            vec![
                (d::CANCEL, Variant::Secondary, PromptAnswer::Cancel),
                (d::SET_UP_NEW, Variant::Secondary, PromptAnswer::SetUpAsNew),
                (
                    d::ITS_AN_UPDATE,
                    Variant::Primary,
                    PromptAnswer::ItsAnUpdate,
                ),
            ],
        ),
        PromptKind::InsideKnownRoot { path, .. } => (
            d::inside_root_title(&file_name(path), &name),
            icons::FILE_EXE,
            vec![d::inside_root_body(&name)],
            vec![
                (d::CANCEL, Variant::Secondary, PromptAnswer::Cancel),
                (d::SET_UP_NEW, Variant::Secondary, PromptAnswer::SetUpAsNew),
                (
                    d::RUN_WITH_SETTINGS,
                    Variant::Primary,
                    PromptAnswer::RunWithGameSettings { remember },
                ),
            ],
        ),
        PromptKind::CopyOfKnown { path, .. } => (
            d::copy_title(&name),
            icons::COPY,
            vec![d::copy_body(&file_name(path), &name)],
            vec![
                (d::CANCEL, Variant::Secondary, PromptAnswer::Cancel),
                (d::SET_UP_NEW, Variant::Secondary, PromptAnswer::SetUpAsNew),
                (d::USE_SAME, Variant::Primary, PromptAnswer::UseSameSettings),
            ],
        ),
        PromptKind::GrantTargetChanged { grant, new, .. } => (
            d::GRANT_MOVED_TITLE.to_owned(),
            icons::FOLDER,
            vec![d::grant_moved_body(
                &name,
                &file_name(grant),
                &new.display().to_string(),
            )],
            vec![
                (d::CANCEL, Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::ALLOW_NEW_LOCATION,
                    Variant::Primary,
                    PromptAnswer::AllowNewLocation,
                ),
            ],
        ),
        PromptKind::RuntimeUnavailable { .. } => (
            d::runtime_missing_title(&name),
            icons::LAYERS,
            vec![d::RUNTIME_MISSING_BODY.to_owned()],
            vec![
                (d::CANCEL, Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::USE_DEFAULT,
                    Variant::Primary,
                    PromptAnswer::UseDefaultRuntime,
                ),
            ],
        ),
        PromptKind::FixNeedsNetwork { prefix_tainted, .. } => (
            d::FIX_NETWORK_TITLE.to_owned(),
            icons::GLOBE,
            {
                let mut v = vec![d::fix_network_body(&name)];
                if *prefix_tainted {
                    v.push(d::TAINTED.to_owned());
                }
                v
            },
            vec![
                (d::NOT_NOW, Variant::Secondary, PromptAnswer::NotNow),
                (
                    d::TURN_OFF_FIXES,
                    Variant::Secondary,
                    PromptAnswer::TurnOffGameFixes,
                ),
                (
                    d::ALLOW_ONCE,
                    Variant::Primary,
                    PromptAnswer::AllowNetworkOnce,
                ),
            ],
        ),
        PromptKind::ToolNeedsNetwork { prefix_tainted, .. } => (
            d::TOOL_NETWORK_TITLE.to_owned(),
            icons::GLOBE,
            {
                let mut v = vec![d::tool_network_body(&name)];
                if *prefix_tainted {
                    v.push(d::TAINTED.to_owned());
                }
                v
            },
            vec![
                (d::CANCEL, Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::ALLOW_ONCE,
                    Variant::Primary,
                    PromptAnswer::AllowNetworkOnce,
                ),
            ],
        ),
        PromptKind::InstallerFinished { programs, .. } => {
            let path = programs
                .get(choice)
                .or(programs.first())
                .map(|c| c.path.clone())
                .unwrap_or_default();
            let mut acts = vec![
                (
                    d::KEEP_AS_INSTALLER,
                    Variant::Secondary,
                    PromptAnswer::KeepAsInstaller,
                ),
                (
                    d::REMOVE_ENTRY,
                    Variant::Secondary,
                    PromptAnswer::RemoveGame,
                ),
            ];
            if !programs.is_empty() {
                acts.push((
                    d::USE_PROGRAM,
                    Variant::Primary,
                    PromptAnswer::PickProgram { path },
                ));
            }
            (
                d::INSTALLER_TITLE.to_owned(),
                icons::DOWNLOAD,
                vec![d::installer_body(&name)],
                acts,
            )
        }
        PromptKind::ConfirmBroadGrant { path, reason, .. } => (
            d::broad_title(&file_name(path)),
            icons::FOLDER,
            vec![d::broad_reason(reason), d::BROAD_REMINDER.to_owned()],
            vec![
                (d::CANCEL, Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::SHARE_ANYWAY,
                    Variant::Secondary,
                    PromptAnswer::AllowAnyway,
                ),
            ],
        ),
        PromptKind::RemoveRunningGame { .. } => (
            d::running_remove_title(&name),
            icons::STOP,
            vec![d::RUNNING_REMOVE_BODY.to_owned()],
            vec![
                (d::CANCEL, Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::STOP_AND_REMOVE,
                    Variant::Danger,
                    PromptAnswer::StopAndRemove,
                ),
            ],
        ),
    };
    let actions: Vec<Action<'_>> = acts
        .iter()
        .map(|(l, v, _)| Action {
            label: l,
            variant: *v,
        })
        .collect();
    let o = rich_dialog(ctx, id, Some(icon), &title, &actions, 0, |ui| {
        for line in &body {
            text(ui, line);
        }
        match &p.kind {
            PromptKind::InsideKnownRoot { .. } => {
                toggle(ui, &mut remember, d::REMEMBER, None);
            }
            PromptKind::InstallerFinished { programs, .. } => {
                let pal = theme::theme(ui.ctx()).palette.clone();
                for (i, prog) in programs.iter().enumerate() {
                    let label = d::program_choice(&prog.label, prog.size);
                    let r = ui.radio(choice == i, Type::Body.rich(&label).color(pal.text_primary));
                    if r.clicked() {
                        choice = i;
                    }
                }
            }
            _ => {}
        }
    });
    state.dialogs.form.remember = remember;
    state.dialogs.form.choice = choice;
    match o {
        DialogOutcome::Open => false,
        DialogOutcome::Chose(i) => {
            if let Some((_, _, a)) = acts.into_iter().nth(i) {
                let a = match a {
                    PromptAnswer::RunWithGameSettings { .. } => {
                        PromptAnswer::RunWithGameSettings { remember }
                    }
                    other => other,
                };
                answer(env, p, a);
            }
            true
        }
        DialogOutcome::Cancelled => {
            let safe = acts.first().map_or(PromptAnswer::Cancel, |a| a.2.clone());
            answer(env, p, safe);
            true
        }
    }
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn error(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, e: &UserError) -> bool {
    let mut details = state.dialogs.form.details;
    let more = state.dialogs.more_errors();
    let recov: Vec<RecoveryAction> = e
        .actions
        .iter()
        .copied()
        .filter(|a| !matches!(a, RecoveryAction::Dismiss | RecoveryAction::CopyDetails))
        .take(3)
        .collect();
    let labels: Vec<&str> = recov.iter().map(|a| strings::errors::action(*a)).collect();
    let mut actions = vec![secondary(d::CLOSE)];
    for (i, l) in labels.iter().enumerate() {
        actions.push(Action {
            label: l,
            variant: if i == 0 {
                Variant::Primary
            } else {
                Variant::Secondary
            },
        });
    }
    // Focus starts on the primary recovery action when there is one.
    let default = if recov.is_empty() { 0 } else { 1 };
    let o = rich_dialog(
        ctx,
        id,
        Some(icons::WARNING),
        &e.title,
        &actions,
        default,
        |ui| {
            let p = theme::theme(ui.ctx()).palette.clone();
            text(ui, &e.message);
            let label = if details {
                d::HIDE_DETAILS
            } else {
                d::SHOW_DETAILS
            };
            ui.horizontal(|ui| {
                if ui
                    .add(
                        crate::widgets::button::Button::ghost(label).icon(if details {
                            icons::CARET_DOWN
                        } else {
                            icons::CARET_RIGHT
                        }),
                    )
                    .clicked()
                {
                    details = !details;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(Type::Caption.rich(e.code.as_str()).color(p.text_muted));
                });
            });
            if details {
                let mut t = strings::errors::copy_text(e);
                let r = ui.add(
                    egui::TextEdit::multiline(&mut t)
                        .font(Type::Mono.font_id())
                        .desired_rows(4)
                        .desired_width(f32::INFINITY)
                        .interactive(true),
                );
                crate::a11y::labelled(
                    ui.ctx(),
                    r.id,
                    egui::Role::MultilineTextInput,
                    d::SHOW_DETAILS,
                );
                if ui
                    .add(crate::widgets::button::Button::ghost(d::COPY).icon(icons::COPY))
                    .clicked()
                {
                    ui.ctx().copy_text(strings::errors::copy_text(e));
                }
            }
            if more > 0 {
                ui.label(
                    Type::Caption
                        .rich(d::more_problems(more))
                        .color(p.text_muted),
                );
            }
        },
    );
    state.dialogs.form.details = details;
    match o {
        DialogOutcome::Open => false,
        DialogOutcome::Chose(i) if i > 0 => {
            if let Some(a) = recov.get(i - 1) {
                actions::recover(state, env, e, *a);
            }
            true
        }
        _ => {
            let _ = ToastKind::Info;
            true
        }
    }
}
