//! Dialogs (script §16 "DIALOGS", UI-027): the confirmations, one dialog per prompt kind of
//! LOGIC_SPECS §4, and the error dialog built from a `UserError` with a queue ("1 more
//! problem"). Every dialog is the design's one dialog ([`crate::widgets::modal`]): title,
//! a sentence, a body of switch rows, pick rows or a detail block, and boxed actions with
//! the main one rightmost. The action that has focus when a dialog opens is its first, the
//! safe choice; `Enter` activates it and `Esc` cancels.

pub mod artwork;
pub mod shortcuts;
pub mod tags;

use easyplay_core::error::{RecoveryAction, UserError};
use easyplay_core::model::{PromptAnswer, PromptKind, PromptRequest};
use egui::{Context, Id, Sense, Ui, pos2, vec2};

use super::Env;
use super::actions;
use crate::app::UiState;
use crate::bridge::{Command, UiCommand};
use crate::route::{DialogId, Overlay};
use crate::strings;
use crate::strings::dialogs as d;
use crate::theme::{self, Type};
use crate::vm::dialogs::{Confirm, Dialog};
use crate::widgets::button::Variant;
use crate::widgets::choice::{PickKind, checkbox, pick};
use crate::widgets::modal::{Action, DialogOutcome, Spec, choose, detail_block, fine_print};
use crate::widgets::rows::SettingRow;
use crate::widgets::toggle::switch;

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

/// A setting row with a switch at its right end. Returns whether it flipped.
fn switch_row(ui: &mut Ui, on: &mut bool, label: &str, help: Option<&str>, enabled: bool) -> bool {
    SettingRow::new(label)
        .help(help.unwrap_or_default())
        .enabled(enabled)
        .show(ui, |ui| switch(ui, on, label).changed())
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
        Dialog::Chat(c) => super::chat::dialog(ctx, id, state, env, c),
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
            let one = ids.len() == 1;
            let title = if one {
                d::remove_title(&game_name(env, ids.first().copied()))
            } else {
                d::remove_many_title(ids.len())
            };
            let (text, verb) = if one {
                (d::REMOVE_BODY(), d::REMOVE())
            } else {
                (d::REMOVE_BODY_MANY(), d::REMOVE_MANY())
            };
            let o = choose(
                ctx,
                id,
                Spec::new(&title).text(text),
                &[secondary(d::CANCEL()), danger(verb)],
                0,
                |ui| {
                    switch_row(
                        ui,
                        &mut options.delete_private_storage,
                        d::REMOVE_STORAGE(),
                        Some(d::REMOVE_STORAGE_HINT()),
                        true,
                    );
                    // Saves live in that storage: keeping them means something only while
                    // it is deleted.
                    let deleting = options.delete_private_storage;
                    switch_row(
                        ui,
                        &mut options.keep_save_data,
                        d::KEEP_SAVES(),
                        None,
                        deleting,
                    );
                    switch_row(
                        ui,
                        &mut options.delete_shortcuts,
                        d::REMOVE_SHORTCUTS(),
                        None,
                        true,
                    );
                    switch_row(ui, &mut options.keep_history, d::KEEP_HISTORY(), None, true);
                },
            );
            if let Some(Dialog::Confirm(Confirm::RemoveGames { options: o2, .. })) =
                state.dialogs.get_mut(dialog)
            {
                *o2 = options;
            }
            if o == DialogOutcome::Chose(1) {
                for g in &ids {
                    env.bridge.send(Command::RemoveGame { id: *g, options });
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
            let title = d::reset_title(&name);
            let o = choose(
                ctx,
                id,
                Spec::new(&title).text(d::RESET_TEXT()),
                &[secondary(d::CANCEL()), danger(d::RESET())],
                0,
                |ui| {
                    switch_row(ui, &mut options.keep_save_data, d::KEEP_SAVES(), None, true);
                },
            );
            if let Some(Dialog::Confirm(Confirm::ResetPrefix { options: o2, .. })) =
                state.dialogs.get_mut(dialog)
            {
                *o2 = options;
            }
            if o == DialogOutcome::Chose(1) {
                env.bridge.send(Command::ResetPrefix { id: game, options });
            }
            o != DialogOutcome::Open
        }
        Confirm::DiscardChanges(after) => {
            // Keeping the edits is the main action and has focus.
            let o = choose(
                ctx,
                id,
                Spec::new(d::DISCARD_TITLE()).text(d::DISCARD_BODY()),
                &[secondary(d::DISCARD()), primary(d::KEEP_EDITING())],
                1,
                |_| {},
            );
            if o == DialogOutcome::Chose(0) {
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
            let text = d::widen_text(preset, running);
            let o = choose(
                ctx,
                id,
                Spec::new(&title).text(&text),
                &[secondary(d::CANCEL()), primary(d::ALLOW())],
                0,
                |_| {},
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
            let title = d::remove_layer_title(&label);
            let text = match size {
                Some(s) => format!("{} {}", d::frees(s), d::layer_users(used_by)),
                None => d::layer_users(used_by),
            };
            let o = choose(
                ctx,
                id,
                Spec::new(&title).text(&text),
                &[secondary(d::CANCEL()), danger(d::REMOVE_LAYER())],
                0,
                |_| {},
            );
            if o == DialogOutcome::Chose(1) {
                env.bridge.send(Command::RemoveRuntime { name });
            }
            o != DialogOutcome::Open
        }
        Confirm::ClearStatistics | Confirm::ResetGameStatistics(_) => {
            let game = match c {
                Confirm::ResetGameStatistics(g) => Some(g),
                _ => None,
            };
            let (title, text, verb) = match game {
                Some(g) => {
                    let name = game_name(env, Some(g));
                    (
                        d::clear_game_title(&name),
                        d::clear_game_text(&name),
                        d::CLEAR(),
                    )
                }
                None => (
                    d::CLEAR_TITLE().to_owned(),
                    d::CLEAR_ALL_TEXT().to_owned(),
                    d::CLEAR_ALL(),
                ),
            };
            let o = choose(
                ctx,
                id,
                Spec::new(&title).text(&text),
                &[secondary(d::CANCEL()), danger(verb)],
                0,
                |_| {},
            );
            if o == DialogOutcome::Chose(1) {
                env.bridge.send(match game {
                    Some(g) => UiCommand::Core(Command::ResetGameStatistics { id: g }),
                    None => UiCommand::Core(Command::ClearStatistics),
                });
            }
            o != DialogOutcome::Open
        }
        Confirm::DeveloperMode => {
            let text = format!("{} {}", d::DEV_BODY(), d::DEV_UNSAFE());
            let o = choose(
                ctx,
                id,
                Spec::new(d::DEV_TITLE()).text(&text),
                &[secondary(d::CANCEL()), primary(d::TURN_ON())],
                0,
                |_| {},
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

/// Title, text and (label, emphasis, answer) actions of a prompt dialog.
type PromptView<'a> = (String, String, Vec<(&'a str, Variant, PromptAnswer)>);

fn answer(env: &mut Env<'_>, p: &PromptRequest, a: PromptAnswer) {
    debug_assert!(p.kind.accepts(&a));
    env.bridge.send(Command::AnswerPrompt {
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
    let lines = |v: &[String]| v.join(" ");
    let (title, text, acts): PromptView<'_> = match &p.kind {
        PromptKind::ExeChanged { old, new, .. } => (
            d::exe_changed_title(&name),
            lines(&[
                d::exe_changed_sizes(old.size, new.size),
                d::EXE_CHANGED_BODY().to_owned(),
            ]),
            vec![
                (d::CANCEL(), Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::SET_UP_NEW(),
                    Variant::Secondary,
                    PromptAnswer::SetUpAsNew,
                ),
                (
                    d::ITS_AN_UPDATE(),
                    Variant::Primary,
                    PromptAnswer::ItsAnUpdate,
                ),
            ],
        ),
        PromptKind::InsideKnownRoot { path, .. } => (
            d::inside_root_title(&file_name(path), &name),
            d::inside_root_body(&name),
            vec![
                (d::CANCEL(), Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::SET_UP_NEW(),
                    Variant::Secondary,
                    PromptAnswer::SetUpAsNew,
                ),
                (
                    d::RUN_WITH_SETTINGS(),
                    Variant::Primary,
                    PromptAnswer::RunWithGameSettings { remember },
                ),
            ],
        ),
        PromptKind::CopyOfKnown { path, .. } => (
            d::copy_title(&name),
            d::copy_body(&file_name(path), &name),
            vec![
                (d::CANCEL(), Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::SET_UP_NEW(),
                    Variant::Secondary,
                    PromptAnswer::SetUpAsNew,
                ),
                (
                    d::USE_SAME(),
                    Variant::Primary,
                    PromptAnswer::UseSameSettings,
                ),
            ],
        ),
        PromptKind::GrantTargetChanged { grant, new, .. } => (
            d::GRANT_MOVED_TITLE().to_owned(),
            d::grant_moved_body(&name, &file_name(grant), &new.display().to_string()),
            vec![
                (d::CANCEL(), Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::ALLOW_NEW_LOCATION(),
                    Variant::Primary,
                    PromptAnswer::AllowNewLocation,
                ),
            ],
        ),
        PromptKind::RuntimeUnavailable { .. } => (
            d::runtime_missing_title(&name),
            d::RUNTIME_MISSING_BODY().to_owned(),
            vec![
                (d::CANCEL(), Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::USE_DEFAULT(),
                    Variant::Primary,
                    PromptAnswer::UseDefaultRuntime,
                ),
            ],
        ),
        PromptKind::FixNeedsNetwork { prefix_tainted, .. } => (
            d::FIX_NETWORK_TITLE().to_owned(),
            {
                let mut v = vec![d::fix_network_body(&name)];
                if *prefix_tainted {
                    v.push(d::TAINTED().to_owned());
                }
                lines(&v)
            },
            vec![
                (d::NOT_NOW(), Variant::Secondary, PromptAnswer::NotNow),
                (
                    d::TURN_OFF_FIXES(),
                    Variant::Secondary,
                    PromptAnswer::TurnOffGameFixes,
                ),
                (
                    d::ALLOW_ONCE(),
                    Variant::Primary,
                    PromptAnswer::AllowNetworkOnce,
                ),
            ],
        ),
        PromptKind::ToolNeedsNetwork { prefix_tainted, .. } => (
            d::TOOL_NETWORK_TITLE().to_owned(),
            {
                let mut v = vec![d::tool_network_body(&name)];
                if *prefix_tainted {
                    v.push(d::TAINTED().to_owned());
                }
                lines(&v)
            },
            vec![
                (d::CANCEL(), Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::ALLOW_ONCE(),
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
                    d::KEEP_AS_INSTALLER(),
                    Variant::Secondary,
                    PromptAnswer::KeepAsInstaller,
                ),
                (
                    d::REMOVE_ENTRY(),
                    Variant::Secondary,
                    PromptAnswer::RemoveGame,
                ),
            ];
            if !programs.is_empty() {
                acts.push((
                    d::USE_PROGRAM(),
                    Variant::Primary,
                    PromptAnswer::PickProgram { path },
                ));
            }
            (
                d::INSTALLER_TITLE().to_owned(),
                d::installer_body(&name),
                acts,
            )
        }
        PromptKind::ConfirmBroadGrant { path, reason, .. } => (
            d::broad_title(&file_name(path)),
            lines(&[d::broad_reason(reason), d::BROAD_REMINDER().to_owned()]),
            vec![
                (d::CANCEL(), Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::SHARE_ANYWAY(),
                    Variant::Secondary,
                    PromptAnswer::AllowAnyway,
                ),
            ],
        ),
        PromptKind::RemoveRunningGame { .. } => (
            d::running_remove_title(&name),
            d::RUNNING_REMOVE_BODY().to_owned(),
            vec![
                (d::CANCEL(), Variant::Secondary, PromptAnswer::Cancel),
                (
                    d::STOP_AND_REMOVE(),
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
    let o = choose(
        ctx,
        id,
        Spec::new(&title).text(&text),
        &actions,
        0,
        |ui| match &p.kind {
            PromptKind::InsideKnownRoot { .. } => {
                checkbox(ui, &mut remember, d::REMEMBER());
            }
            PromptKind::InstallerFinished { programs, .. } => {
                for (i, prog) in programs.iter().enumerate() {
                    let spoken = d::program_choice(&prog.label, prog.size);
                    let (r, ()) = pick(ui, PickKind::Radio, choice == i, &spoken, |ui| {
                        name_and_size(ui, &prog.label, &strings::fmt::bytes(prog.size));
                    });
                    if r.clicked() {
                        choice = i;
                    }
                }
            }
            _ => {}
        },
    );
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

/// One line of a pick row: `name` in `t1` at the left, `size` in `t3` at the right.
pub(crate) fn name_and_size(ui: &mut Ui, name: &str, size: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width();
    let h = Type::Body.line_height();
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let sg = Type::Body.numeric_galley(ui, size, p.t3);
    let sw = sg.size().x;
    let ng = Type::Body.galley(ui, name, p.t1, (w - sw - layout_gap()).max(1.0), 1);
    ui.painter().galley(
        pos2(rect.left(), rect.center().y - ng.size().y / 2.0),
        ng,
        p.t1,
    );
    ui.painter().galley(
        pos2(rect.right() - sw, rect.center().y - sg.size().y / 2.0),
        sg,
        p.t3,
    );
}

fn layout_gap() -> f32 {
    theme::tokens::control::PICK_GAP
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

fn error(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, e: &UserError) -> bool {
    let more = state.dialogs.more_errors();
    let recov: Vec<RecoveryAction> = e
        .actions
        .iter()
        .copied()
        .filter(|a| !matches!(a, RecoveryAction::Dismiss | RecoveryAction::CopyDetails))
        .take(3)
        .collect();
    // Close first, then the other fixes, then the main fix (the first) rightmost.
    let mut order: Vec<Option<usize>> = vec![None];
    order.extend((1..recov.len()).map(Some));
    if !recov.is_empty() {
        order.push(Some(0));
    }
    let actions: Vec<Action<'_>> = order
        .iter()
        .map(|o| match o {
            None => secondary(d::CLOSE()),
            Some(0) => primary(strings::errors::action(recov[0])),
            Some(i) => secondary(strings::errors::action(recov[*i])),
        })
        .collect();
    let detail = match &e.details {
        Some(x) if !x.trim().is_empty() => format!("{}\n{}", x.trim_end(), e.code.as_str()),
        _ => e.code.as_str().to_owned(),
    };
    let o = choose(
        ctx,
        id,
        Spec::new(&e.title).text(&e.message),
        &actions,
        0,
        |ui| {
            detail_block(ui, &detail, false);
            if more > 0 {
                ui.add_space(theme::tokens::overlay::PRE[0]);
                fine_print(ui, &d::more_problems(more));
            }
        },
    );
    match o {
        DialogOutcome::Open => false,
        DialogOutcome::Chose(i) => {
            if let Some(Some(r)) = order.get(i)
                && let Some(a) = recov.get(*r)
            {
                actions::recover(state, env, e, *a);
            }
            true
        }
        DialogOutcome::Cancelled => true,
    }
}
