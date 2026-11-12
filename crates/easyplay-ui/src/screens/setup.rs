//! First-time setup sheet (SCREEN_SPECS §6, UI-015, UF-2): identity block, the protection
//! decision with its live access summary, hints, *More options*, the queue indicator, footer
//! variants by origin, validation, and the launch progress view after *Play*. The primary
//! button has initial focus so `Enter` alone completes the common case.

use easyplay_core::model::{
    AudioAccess, BasePreset, ControllerAccess, EntryKind, GameDirAccess, GrantOrigin,
    SaveDataLocation,
};
use egui::{Align, Context, CornerRadius, Id, Key, Layout, Modifiers, Popup, Sense, Ui, vec2};

use super::Env;
use super::details::launch_panel;
use super::details::permissions::{USER_DIRS, issue_line, issue_text, preset_cards, section};
use super::details::statement_row;
use crate::app::UiState;
use crate::bridge::{AfterCreate, UiCommand};
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::route::Overlay;
use crate::strings;
use crate::strings::setup as s;
use crate::theme::tokens::{icon as icon_size, radius, space};
use crate::theme::{self, Type, icons};
use crate::vm::setup::{SetupPhase, SetupVm};
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::chip::{ChipKind, chip};
use crate::widgets::context_menu::item;
use crate::widgets::cover::{CoverSource, paint_cover};
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::sheet::sheet;
use crate::widgets::text_field::TextField;
use crate::widgets::toggle::toggle;

fn reassess(v: &mut SetupVm, env: &Env<'_>) {
    v.assessment = env
        .bridge
        .assess(None, &v.name, v.draft.root.as_deref(), &v.draft.policy);
}

fn set_preset(v: &mut SetupVm, env: &Env<'_>, b: BasePreset) {
    v.draft.policy = env.bridge.preset_policy(b, &v.draft.policy);
    reassess(v, env);
    crate::a11y::Announcer::announce(
        &env.ctx,
        s::announce_preset(b, v.assessment.statements.first().map(|x| x.text.as_str())),
        crate::a11y::Politeness::Polite,
    );
}

fn folder_label(v: &SetupVm, home: &std::path::Path) -> String {
    match &v.draft.root {
        Some(r) => s::breadcrumb(r, home),
        None => s::FILE_ONLY.to_owned(),
    }
}

fn identity(ui: &mut Ui, v: &mut SetupVm, state_picks: &mut Vec<PickPurpose>, env: &Env<'_>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(88.0, 132.0), Sense::hover());
        paint_cover(
            ui.painter(),
            rect,
            CornerRadius::same(radius::MD),
            CoverSource::Placeholder,
            &v.name,
            &theme,
        );
        ui.add_space(space::S4);
        ui.vertical(|ui| {
            let err = v.name_error().map(|_| s::NAME_REQUIRED);
            TextField::new(&mut v.name, "setup.name")
                .label(s::NAME)
                .error(err)
                .width(ui.available_width().min(420.0))
                .show(ui);
            ui.add_space(space::S2);
            ui.label(Type::Caption.rich(s::GAME_FOLDER).color(p.text_secondary));
            let home = env.bridge.host().home.clone();
            let label = folder_label(v, &home);
            let r = ui.add(
                Button::new(&label)
                    .icon(icons::FOLDER)
                    .accessible_name(s::folder_button(&label)),
            );
            let mut choose = None;
            Popup::menu(&r).show(|ui| {
                for c in v.original.proposal.candidates.clone() {
                    let l = s::breadcrumb(&c, &home);
                    if item(
                        ui,
                        Some(icons::FOLDER),
                        &l,
                        None,
                        v.draft.root.as_ref() == Some(&c),
                    )
                    .clicked()
                    {
                        choose = Some(Some(c));
                    }
                }
                if item(
                    ui,
                    Some(icons::FILE_EXE),
                    s::FILE_ONLY,
                    None,
                    v.draft.root.is_none(),
                )
                .clicked()
                {
                    choose = Some(None);
                }
                if item(ui, Some(icons::FOLDER_PLUS), s::CHOOSE_FOLDER, None, false).clicked() {
                    state_picks.push(PickPurpose::SetupRoot);
                }
            });
            if let Some(c) = choose {
                v.set_root(c);
                reassess(v, env);
            }
            if v.draft.root.is_none() {
                ui.add(
                    egui::Label::new(Type::Caption.rich(s::LOOSE_FILE).color(p.text_secondary))
                        .wrap(),
                );
            }
        });
    });
}

fn summary(ui: &mut Ui, v: &SetupVm) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.add_space(space::S2);
    ui.label(Type::BodyStrong.rich(s::WILL_BE_ABLE).color(p.text_primary));
    let list = ui.scope(|ui| {
        for st in &v.assessment.statements {
            statement_row(ui, st, None);
        }
    });
    crate::a11y::labelled(
        ui.ctx(),
        list.response.id,
        egui::Role::List,
        s::WILL_BE_ABLE,
    );
    ui.add_space(space::S1);
    ui.horizontal_wrapped(|ui| {
        ui.add(egui::Label::new(Type::Caption.rich(s::HONESTY).color(p.text_secondary)).wrap());
        let more = ui.add(
            Button::new(s::LEARN_MORE)
                .variant(Variant::Link)
                .size(Size::Sm),
        );
        Popup::menu(&more).width(360.0).show(|ui| {
            ui.label(Type::Body.rich(s::HONESTY_MORE).color(p.text_secondary));
        });
    });
}

fn hints(ui: &mut Ui, v: &SetupVm) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let mut hint = |icon, text: &str| {
        ui.horizontal_wrapped(|ui| {
            ui.label(icons::icon(icon, icon_size::INLINE).color(p.info));
            ui.add(egui::Label::new(Type::Body.rich(text).color(p.text_secondary)).wrap());
        });
    };
    if v.is_installer() {
        hint(icons::DOWNLOAD, s::INSTALLER_HINT);
    }
    if v.draft.hints.online_capable && v.base() == BasePreset::Offline {
        hint(icons::GLOBE, s::ONLINE_HINT);
    }
}

fn more_options(ui: &mut Ui, v: &mut SetupVm, picks: &mut Vec<PickPurpose>, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let segs = |labels: &[&'static str]| -> Vec<Segment<'static>> {
        labels
            .iter()
            .map(|l| Segment {
                icon: None,
                label: l,
            })
            .collect()
    };
    let mut changed = false;
    section(ui, s::FILES, None);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::S2, space::S2);
        for d in USER_DIRS {
            let on = v
                .draft
                .policy
                .folders
                .iter()
                .any(|f| f.origin == GrantOrigin::WellKnown(d));
            let name = crate::demo::user_dir_name(d);
            let r = chip(
                ui,
                ChipKind::Filter { selected: on },
                name,
                Some(icons::FOLDER),
                false,
            );
            crate::a11y::toggle(&r.response, &strings::details::share_toggle(name), on);
            if r.response.clicked()
                && let Some(path) = super::details::permissions::user_dir_path(env, d)
            {
                let mut e = crate::vm::details::PolicyEditor::new(
                    v.draft.policy.clone(),
                    v.assessment.clone(),
                );
                e.toggle_user_dir(d, path, env.now);
                v.draft.policy = e.draft;
                changed = true;
            }
        }
    });
    for f in v
        .draft
        .policy
        .folders
        .iter()
        .filter(|f| f.origin == GrantOrigin::UserChosen)
    {
        ui.label(
            Type::Body
                .rich(f.path.display().to_string())
                .color(p.text_secondary),
        );
    }
    if let Some(e) = &v.folder_error {
        issue_line(ui, &strings::details::rejected(e), true);
    }
    if ui
        .add(
            Button::new(strings::details::ADD_FOLDER)
                .icon(icons::FOLDER_PLUS)
                .size(Size::Sm),
        )
        .clicked()
    {
        picks.push(PickPurpose::FolderGrant(None));
    }
    if v.draft.root.is_some() {
        ui.label(
            Type::Body
                .rich(strings::details::GAME_FOLDER_ACCESS)
                .color(p.text_primary),
        );
        let mut gd = match v.draft.policy.game_dir {
            GameDirAccess::ReadWrite => 0,
            GameDirAccess::ReadOnly | GameDirAccess::FileOnly => 1,
            GameDirAccess::Protected => 2,
        };
        if segmented(
            ui,
            strings::details::GAME_FOLDER_ACCESS,
            &mut gd,
            &segs(&[
                strings::details::CAN_CHANGE,
                strings::details::READ_ONLY,
                strings::details::PROTECTED,
            ]),
            false,
        )
        .changed()
        {
            v.draft.policy.game_dir = [
                GameDirAccess::ReadWrite,
                GameDirAccess::ReadOnly,
                GameDirAccess::Protected,
            ][gd];
            changed = true;
        }
    }
    section(ui, strings::details::SAVE_DATA, None);
    let mut sd = usize::from(matches!(
        v.draft.policy.save_data,
        SaveDataLocation::External { .. }
    ));
    if segmented(
        ui,
        strings::details::SAVE_DATA,
        &mut sd,
        &segs(&[
            strings::details::PRIVATE_STORAGE,
            strings::details::CHOOSE_FOLDER,
        ]),
        false,
    )
    .changed()
    {
        if sd == 0 {
            v.draft.policy.save_data = SaveDataLocation::Internal;
            changed = true;
        } else {
            picks.push(PickPurpose::SaveFolder(None));
        }
    }
    section(ui, strings::details::DEVICES, None);
    let mut sound = v.draft.policy.devices.audio == AudioAccess::Full;
    if toggle(ui, &mut sound, strings::details::SOUND, None).changed() {
        v.draft.policy.devices.audio = if sound {
            AudioAccess::Full
        } else {
            AudioAccess::None
        };
        changed = true;
    }
    let mut ctrl = v.draft.policy.devices.controllers != ControllerAccess::None;
    if toggle(ui, &mut ctrl, strings::details::CONTROLLERS, None).changed() {
        v.draft.policy.devices.controllers = if ctrl {
            ControllerAccess::Detected
        } else {
            ControllerAccess::None
        };
        changed = true;
    }
    changed |= toggle(
        ui,
        &mut v.draft.policy.devices.camera,
        strings::details::CAMERA,
        None,
    )
    .changed();
    section(ui, strings::details::LAYER, None);
    let mut rt = v.draft.runtime.clone();
    ui.label(
        Type::Body
            .rich(super::details::runtime::runtime_label(&rt, env))
            .color(p.text_secondary),
    );
    let _ = &mut rt;
    section(ui, s::LAUNCH, None);
    let args_err = easyplay_core::model::split_args(&v.args)
        .err()
        .map(|_| strings::details::ARGS_UNBALANCED);
    TextField::new(&mut v.args, "setup.args")
        .label(strings::details::ARGUMENTS)
        .mono()
        .error(args_err)
        .show(ui);
    section(ui, s::TYPE, None);
    let mut k = match v.draft.kind {
        EntryKind::Game => 0,
        EntryKind::Installer => 1,
        EntryKind::Tool => 2,
    };
    if segmented(
        ui,
        s::TYPE,
        &mut k,
        &segs(&[s::KIND_GAME, s::KIND_INSTALLER, s::KIND_TOOL]),
        false,
    )
    .changed()
    {
        v.draft.kind = [EntryKind::Game, EntryKind::Installer, EntryKind::Tool][k];
    }
    if changed {
        v.reclassify();
        reassess(v, env);
    }
}

fn editing_body(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let mut picks = Vec::new();
    let Some(v) = state.setup.as_mut() else {
        return;
    };
    let (i, n) = v.context.queue;
    if n > 1 {
        ui.horizontal(|ui| {
            ui.label(Type::Caption.rich(s::queue(i, n)).color(p.text_muted));
        });
    }
    identity(ui, v, &mut picks, env);
    ui.add_space(space::S4);
    ui.horizontal(|ui| {
        let h = ui.label(
            Type::Heading
                .rich(strings::details::PROTECTION)
                .color(p.text_primary),
        );
        crate::a11y::labelled(
            ui.ctx(),
            h.id,
            egui::Role::Heading,
            strings::details::PROTECTION,
        );
        if v.customised() {
            ui.label(
                Type::Caption
                    .rich(strings::details::custom_based_on(v.base()))
                    .color(p.text_secondary),
            );
            if ui
                .add(
                    Button::new(strings::details::RESET_TO_PRESET)
                        .variant(Variant::Link)
                        .size(Size::Sm),
                )
                .clicked()
            {
                let b = v.base();
                v.draft.policy = env.bridge.preset_policy(b, &v.original.policy);
                reassess(v, env);
            }
        }
    });
    if let Some(b) = preset_cards(ui, v.draft.policy.preset) {
        set_preset(v, env, b);
    }
    hints(ui, v);
    summary(ui, v);
    for issue in v.assessment.issues.clone() {
        let err = issue.severity >= easyplay_core::sandbox::IssueSeverity::Error;
        if issue.severity >= easyplay_core::sandbox::IssueSeverity::Warning {
            issue_line(ui, issue_text(&issue.message), err);
        }
    }
    if v.warnings() > 0 {
        ui.checkbox(&mut v.acknowledged, s::I_UNDERSTAND);
    }
    ui.add_space(space::S3);
    if ui
        .add(Button::ghost(s::MORE_OPTIONS).icon(if v.more {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        }))
        .clicked()
    {
        v.more = !v.more;
    }
    if v.more {
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: space::S6 as i8,
                ..Default::default()
            })
            .show(ui, |ui| more_options(ui, v, &mut picks, env));
    }
    let r = ui.min_rect();
    state.dnd.target(r, DropTarget::FolderList(None));
    state.picks.extend(picks);
}

fn launching_body(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let Some(v) = state.setup.as_ref() else {
        return;
    };
    let SetupPhase::Launching { game } = v.phase.clone() else {
        return;
    };
    let name = v.name.clone();
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(64.0, 96.0), Sense::hover());
        paint_cover(
            ui.painter(),
            rect,
            CornerRadius::same(radius::MD),
            CoverSource::Placeholder,
            &name,
            &theme::theme(ui.ctx()),
        );
        ui.label(Type::Title.rich(&name).color(p.text_primary));
    });
    ui.add_space(space::S3);
    let Some(g) = game else {
        ui.label(
            Type::Body
                .rich(strings::launch::CHECKING)
                .color(p.text_secondary),
        );
        return;
    };
    if let Some(l) = state.launches.active(g).cloned() {
        if launch_panel::progress_row(ui, &l, env.mono) {
            env.bridge.send(UiCommand::CancelLaunch { id: g });
        }
    } else if state.launches.failures.contains_key(&g) {
        let err = state.launches.failures.get(&g).cloned();
        launch_panel::problem_panel(ui, state, env, g, err);
    } else if state
        .launches
        .games
        .get(&g)
        .is_some_and(|l| matches!(l.state, easyplay_core::launch::LaunchState::Running { .. }))
    {
        ui.label(Type::Body.rich(strings::launch::RUNNING).color(p.success));
    }
}

/// Show the sheet.
pub fn show(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) {
    let Some(v) = state.setup.as_ref() else {
        state.nav.close(Overlay::Setup);
        return;
    };
    let editing = v.phase == SetupPhase::Editing;
    let unsaved = editing && v.draft != v.original;
    // Alt+1/2/3 select presets.
    if editing {
        let pick = ctx.input_mut(|i| {
            [Key::Num1, Key::Num2, Key::Num3]
                .iter()
                .position(|k| i.consume_key(Modifiers::ALT, *k))
        });
        if let (Some(k), Some(v)) = (pick, state.setup.as_mut()) {
            set_preset(v, env, BasePreset::ALL[k]);
        }
    }
    // The running game closes the progress view on its own.
    if let Some(SetupPhase::Launching { game: Some(g) }) =
        state.setup.as_ref().map(|v| v.phase.clone())
        && state
            .launches
            .games
            .get(&g)
            .is_some_and(|l| matches!(l.state, easyplay_core::launch::LaunchState::Running { .. }))
    {
        crate::widgets::modal::closed(ctx, id);
        state.setup = None;
        state.nav.close(Overlay::Setup);
        return;
    }
    let title = if editing { s::TITLE } else { s::STARTING_TITLE };
    #[derive(PartialEq)]
    enum Foot {
        None,
        Cancel,
        Add,
        Play,
        ApplyRest,
        Background,
    }
    let (ok, plays, installer, batch, focus) =
        state
            .setup
            .as_ref()
            .map_or((false, false, false, false, false), |v| {
                (
                    v.can_continue(),
                    v.primary_plays(),
                    v.is_installer(),
                    v.context.queue.1 > 1,
                    v.focus_primary,
                )
            });
    let r = sheet(
        ctx,
        id,
        title,
        unsaved,
        |ui| {
            if editing {
                editing_body(ui, state, env);
            } else {
                launching_body(ui, state, env);
            }
        },
        |ui| {
            let mut out = Foot::None;
            if !editing {
                if ui.add(Button::new(s::RUN_IN_BACKGROUND)).clicked() {
                    out = Foot::Background;
                }
                return out;
            }
            let reason = (!ok).then_some(s::FIX_TO_CONTINUE);
            let primary_label = if installer {
                strings::play::RUN_INSTALLER
            } else if plays {
                strings::play::PLAY
            } else {
                s::ADD_TO_LIBRARY
            };
            let primary = ui.add(
                Button::primary(primary_label)
                    .icon(if plays || installer {
                        icons::PLAY
                    } else {
                        icons::PLUS
                    })
                    .disabled(reason),
            );
            if focus {
                primary.request_focus();
            }
            if primary.clicked() {
                out = if plays || installer {
                    Foot::Play
                } else {
                    Foot::Add
                };
            }
            let secondary_label = if plays || installer {
                s::ADD_TO_LIBRARY
            } else {
                strings::play::PLAY
            };
            if ui
                .add(Button::new(secondary_label).disabled(reason))
                .clicked()
            {
                out = if plays || installer {
                    Foot::Add
                } else {
                    Foot::Play
                };
            }
            if batch
                && ui
                    .add(Button::ghost(s::USE_FOR_REST).disabled(reason))
                    .clicked()
            {
                out = Foot::ApplyRest;
            }
            if let Some(r) = reason {
                let p = theme::theme(ui.ctx()).palette.clone();
                ui.label(Type::Caption.rich(r).color(p.danger));
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                if ui.add(Button::new(strings::dialogs::CANCEL)).clicked() {
                    out = Foot::Cancel;
                }
            });
            out
        },
    );
    if focus && let Some(v) = state.setup.as_mut() {
        v.focus_primary = false;
    }
    let close = |state: &mut UiState| {
        crate::widgets::modal::closed(ctx, id);
        state.setup = None;
        state.nav.close(Overlay::Setup);
    };
    match r.footer {
        Foot::Play | Foot::Add | Foot::ApplyRest => {
            let Some(v) = state.setup.as_mut() else {
                return;
            };
            let then = match r.footer {
                Foot::Play => AfterCreate::Launch,
                Foot::ApplyRest => AfterCreate::ApplyToRest,
                _ => AfterCreate::Nothing,
            };
            env.bridge.send(UiCommand::CreateGame {
                draft: Box::new(v.finished()),
                then,
            });
            if then == AfterCreate::Launch {
                v.phase = SetupPhase::Launching { game: None };
            } else {
                close(state);
            }
        }
        Foot::Background => close(state),
        Foot::Cancel => {
            env.bridge.send(UiCommand::DiscardDraft);
            close(state);
        }
        Foot::None => {
            if r.close_requested {
                if editing {
                    env.bridge.send(UiCommand::DiscardDraft);
                }
                close(state);
            }
        }
    }
}
