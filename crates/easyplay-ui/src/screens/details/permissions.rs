//! Protection tab (SCREEN_SPECS_2 §8, UI-017) and folder permissions (§9, UI-018): preset
//! selector with "Custom (based on …)", the access summary with added/removed markers,
//! Internet, Files and folders, Save data, Devices, Advanced, and the save bar (in the page).
//! A rejected folder never enters the list: the message names the rule instead.

use std::path::PathBuf;

use easyplay_core::ids::GameId;
use easyplay_core::model::{
    AudioAccess, BasePreset, ControllerAccess, FolderAccess, GameDirAccess, GameSummary,
    GrantOrigin, NetworkAccess, Preset, RemovableAccess, RuntimeIsolation, SaveDataLocation,
    SeccompMode, UserDir,
};
use easyplay_core::sandbox::IssueSeverity;
use egui::{Align, CornerRadius, Layout, Sense, Ui, vec2};

use super::{card, statement_row};
use crate::app::{UiState, assess_draft};
use crate::bridge::{GameEdit, UiCommand};
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::screens::Env;
use crate::strings;
use crate::strings::details as s;
use crate::theme::palette::fade;
use crate::theme::tokens::{alpha, icon as icon_size, radius, space, stroke};
use crate::theme::{self, Type, icons};
use crate::vm::details::PolicyEditor;
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::chip::{ChipKind, chip};
use crate::widgets::path_row::{PathRowState, path_row_with};
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::toggle::toggle;

/// Well-known folders in display order.
pub const USER_DIRS: [UserDir; 6] = [
    UserDir::Downloads,
    UserDir::Documents,
    UserDir::Pictures,
    UserDir::Music,
    UserDir::Videos,
    UserDir::Desktop,
];

/// Where a well-known folder lives on this host.
pub fn user_dir_path(env: &Env<'_>, d: UserDir) -> Option<PathBuf> {
    env.bridge
        .host()
        .user_dirs
        .iter()
        .find(|(x, _)| *x == d)
        .map(|(_, p)| p.clone())
}

fn segs<'a>(labels: &[&'a str]) -> Vec<Segment<'a>> {
    labels
        .iter()
        .map(|l| Segment {
            icon: None,
            label: l,
        })
        .collect()
}

/// Section heading with an optional description.
pub fn section(ui: &mut Ui, title: &str, desc: Option<&str>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.add_space(space::S4);
    let h = ui.label(Type::Heading.rich(title).color(p.text_primary));
    crate::a11y::labelled(ui.ctx(), h.id, egui::Role::Heading, title);
    if let Some(d) = desc {
        ui.add(egui::Label::new(Type::Caption.rich(d).color(p.text_secondary)).wrap());
    }
    ui.add_space(space::S1);
}

/// An inline issue line.
pub fn issue_line(ui: &mut Ui, text: &str, error: bool) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let c = if error { p.danger } else { p.warning };
    ui.horizontal_wrapped(|ui| {
        ui.label(
            icons::icon(
                if error { icons::ERROR } else { icons::WARNING },
                icon_size::INLINE,
            )
            .color(c),
        );
        ui.add(egui::Label::new(Type::Body.rich(text).color(c)).wrap());
    });
}

/// The English fallback of an issue message (`key|text`).
pub fn issue_text(message: &str) -> &str {
    message.split_once('|').map_or(message, |(_, t)| t)
}

/// The three preset option cards. Returns the chosen preset.
pub fn preset_cards(ui: &mut Ui, current: Preset) -> Option<BasePreset> {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let mut out = None;
    let narrow = ui.available_width() < 560.0;
    let n = BasePreset::ALL.len() as f32;
    let gap = space::S3;
    let w = if narrow {
        ui.available_width()
    } else {
        (ui.available_width() - gap * (n - 1.0)) / n
    };
    let group = ui.scope(|ui| {
        let layout = if narrow {
            Layout::top_down(Align::Min)
        } else {
            Layout::left_to_right(Align::Min)
        };
        ui.with_layout(layout, |ui| {
            ui.spacing_mut().item_spacing = vec2(gap, gap);
            for b in BasePreset::ALL {
                let selected = current.base() == b;
                let h = if narrow { 64.0 } else { 92.0 };
                let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
                let fill = if selected {
                    fade(p.accent, alpha::BANNER_FILL)
                } else if resp.hovered() {
                    p.bg_raised
                } else {
                    p.bg_surface
                };
                let border = if selected { p.accent } else { p.border_subtle };
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(radius::MD), fill);
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::same(radius::MD),
                    egui::Stroke::new(if selected { 2.0 } else { stroke::BORDER }, border),
                    egui::StrokeKind::Inside,
                );
                let inner = rect.shrink(space::S3);
                crate::widgets::shield::paint(
                    ui.painter(),
                    egui::pos2(inner.left() + 10.0, inner.top() + 10.0),
                    icon_size::BUTTON,
                    b.into(),
                    false,
                    if selected { p.accent } else { p.text_secondary },
                );
                ui.painter().text(
                    egui::pos2(inner.left() + 28.0, inner.top() + 10.0),
                    egui::Align2::LEFT_CENTER,
                    strings::shield::preset_name(b.into()),
                    Type::BodyStrong.font_id(),
                    p.text_primary,
                );
                let galley = ui.painter().layout(
                    strings::shield::base_description(b).to_owned(),
                    Type::Caption.font_id(),
                    p.text_secondary,
                    inner.width(),
                );
                ui.painter().galley(
                    egui::pos2(inner.left(), inner.top() + 24.0),
                    galley,
                    p.text_secondary,
                );
                if resp.has_focus() {
                    crate::widgets::focus_ring(
                        ui.painter(),
                        rect,
                        CornerRadius::same(radius::MD),
                        &theme,
                    );
                }
                let name = strings::shield::preset_name(b.into());
                crate::a11y::option(&resp, egui::Role::RadioButton, name, selected);
                crate::a11y::describe(&resp, strings::shield::base_description(b));
                if resp.clicked() && !selected {
                    out = Some(b);
                }
            }
        });
    });
    crate::a11y::labelled(
        ui.ctx(),
        group.response.id,
        egui::Role::RadioGroup,
        s::PROTECTION,
    );
    out
}

fn editor(state: &mut UiState) -> Option<&mut PolicyEditor> {
    state.details.protection.as_mut()
}

fn reassess(state: &mut UiState, env: &Env<'_>) {
    let Some(draft) = state.details.protection.as_ref().map(|e| e.draft.clone()) else {
        return;
    };
    let a = assess_draft(env.bridge, &state.details, &draft);
    if let Some(e) = editor(state) {
        // Access summary changes while editing are announced (ACCESSIBILITY §4.3).
        for st in &a.statements {
            if !e.assessment.statements.iter().any(|o| o.text == st.text) {
                crate::a11y::Announcer::announce(
                    &env.ctx,
                    strings::a11y::access_added(&st.text),
                    crate::a11y::Politeness::Polite,
                );
            }
        }
        e.assessment = a;
    }
}

/// Add a folder to a game's draft (or the setup draft); a rejected folder is taken out again
/// and explained.
pub fn share_folder(state: &mut UiState, env: &mut Env<'_>, game: Option<GameId>, path: PathBuf) {
    match game {
        Some(g) if state.details.game == Some(g) => {
            if let Some(e) = editor(state) {
                e.folder_error = None;
                e.add_folder(path.clone(), env.now);
            }
            reassess(state, env);
            reject_denied(state, env, &path);
        }
        Some(_) => {}
        None => {
            if let Some(setup) = &mut state.setup {
                let mut e = PolicyEditor::new(setup.draft.policy.clone(), setup.assessment.clone());
                e.add_folder(path.clone(), env.now);
                let a = env
                    .bridge
                    .assess(None, &setup.name, setup.draft.root.as_deref(), &e.draft);
                if let Some(err) = a.issues.iter().find(|i| {
                    i.severity >= IssueSeverity::Error && i.path.as_deref() == Some(path.as_path())
                }) {
                    setup.folder_error = Some(issue_text(&err.message).to_owned());
                } else {
                    setup.folder_error = None;
                    setup.draft.policy = e.draft;
                    setup.assessment = a;
                }
            }
        }
    }
}

fn reject_denied(state: &mut UiState, env: &Env<'_>, path: &std::path::Path) {
    let Some(e) = editor(state) else { return };
    let denied = e
        .assessment
        .issues
        .iter()
        .find(|i| i.severity >= IssueSeverity::Error && i.path.as_deref() == Some(path))
        .map(|i| issue_text(&i.message).to_owned());
    if let Some(msg) = denied {
        e.draft.folders.retain(|g| g.canonical != path);
        e.reclassify();
        e.folder_error = Some(msg);
        crate::a11y::Announcer::announce(
            &env.ctx,
            e.folder_error.clone().unwrap_or_default(),
            crate::a11y::Politeness::Assertive,
        );
        reassess(state, env);
    }
}

/// *Locate…* for a missing folder: the grant moves to `new`, keeping its access; the new
/// place is validated like a newly shared folder.
pub fn relocate_folder(state: &mut UiState, game: GameId, old: &std::path::Path, new: PathBuf) {
    if state.details.game != Some(game) {
        return;
    }
    if let Some(e) = editor(state)
        && let Some(g) = e.draft.folders.iter_mut().find(|g| g.canonical == old)
    {
        g.path = new.clone();
        g.canonical = new.clone();
        g.origin = GrantOrigin::UserChosen;
        e.reclassify();
    }
    state.shares.push((Some(game), new));
}

/// Save every dirty draft of the page.
pub fn save(state: &mut UiState, env: &mut Env<'_>) {
    let Some(id) = state.details.game else { return };
    let mut edits = Vec::new();
    if let Some(e) = &state.details.protection
        && e.dirty()
        && e.errors() == 0
    {
        edits.push(GameEdit::Policy(Box::new(e.draft.clone())));
    }
    if state.details.compat_dirty() {
        edits.extend(state.details.compat_edits());
    } else if state.details.launch_dirty()
        && let Some(cfg) = state
            .details
            .launch
            .as_ref()
            .and_then(|l| l.resolved().ok())
    {
        edits.push(GameEdit::Launch(Box::new(cfg)));
    }
    if edits.is_empty() {
        return;
    }
    env.bridge.send(UiCommand::UpdateGame { id, edits });
    // Optimistic: the drafts become the saved state.
    if let Some(e) = editor(state)
        && e.dirty()
    {
        let saved = e.draft.clone();
        let a = e.assessment.clone();
        *e = PolicyEditor::new(saved, a);
    }
    if let (Some(d), Some(c)) = (
        state.details.detail.ready().cloned(),
        state.details.compat.clone(),
    ) {
        let mut d = d;
        d.game.runtime = c.runtime.clone();
        d.game.runtime_follow_updates = c.follow;
        d.game.umu_id = c.umu_id.clone();
        d.game.launch.compat = c.compat.clone();
        if let Some(cfg) = state
            .details
            .launch
            .as_ref()
            .and_then(|l| l.resolved().ok())
        {
            d.game.launch = easyplay_core::model::LaunchConfig {
                compat: c.compat,
                ..cfg
            };
        }
        state.details.detail = crate::vm::AsyncValue::Ready(d);
    }
    state.toast(
        strings::feedback::SAVED,
        crate::vm::toasts::ToastKind::Success,
        None,
        env.mono,
    );
}

fn folders_section(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let id = g.id;
    section(ui, s::FILES, None);
    let rect_start = ui.cursor().min;
    // Its own folder.
    ui.horizontal(|ui| {
        ui.label(Type::Body.rich(s::OWN_FOLDER).color(p.text_muted));
        ui.label(icons::icon(icons::FOLDER, icon_size::INLINE).color(p.text_secondary));
        let (name, parent) = s::location(g.root.as_deref(), &g.primary_exe_path);
        ui.label(Type::BodyStrong.rich(name).color(p.text_primary));
        if let Some(parent) = parent {
            ui.label(Type::Body.rich(parent).color(p.text_secondary));
        }
    });
    let Some(e) = editor(state) else { return };
    let mut changed = false;
    // Well-known folders.
    ui.label(Type::Body.rich(s::YOUR_FOLDERS).color(p.text_muted));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::S2, space::S2);
        for d in USER_DIRS {
            let on = e
                .draft
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
            crate::a11y::toggle(&r.response, &s::share_toggle(name), on);
            if r.response.clicked()
                && let Some(path) = user_dir_path(env, d)
            {
                e.toggle_user_dir(d, path, env.now);
                changed = true;
            }
        }
    });
    if let Some(err) = &e.folder_error {
        issue_line(ui, &s::rejected(err), true);
    }
    // Explicit list.
    let mut state_picks: Vec<PickPurpose> = Vec::new();
    let issues = e.assessment.issues.clone();
    let network = e.draft.network;
    let mut remove = None;
    let rows: Vec<(usize, PathBuf, PathBuf, FolderAccess, GrantOrigin)> = e
        .draft
        .folders
        .iter()
        .enumerate()
        .map(|(i, f)| (i, f.path.clone(), f.canonical.clone(), f.access, f.origin))
        .collect();
    if rows.is_empty() {
        ui.label(Type::Body.rich(s::NO_FOLDERS).color(p.text_secondary));
    }
    for (i, path, canonical, access, _origin) in rows {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let loc = s::folder_location(&path, &env.bridge.host().home);
        let mut a = usize::from(access == FolderAccess::ReadWrite);
        let before = a;
        let group = s::folder_row_name(&name, &loc, a == 1);
        let missing = issues.iter().any(|x| {
            x.code == easyplay_core::sandbox::IssueCode::PathMissing
                && (x.path.as_deref() == Some(canonical.as_path())
                    || x.path.as_deref() == Some(path.as_path()))
        });
        let r = path_row_with(
            ui,
            icons::FOLDER,
            &name,
            &loc,
            &path.display().to_string(),
            &group,
            &mut a,
            [s::CAN_READ, s::CAN_CHANGE],
            &s::remove_folder(&name),
            PathRowState {
                missing: missing.then_some((s::FOLDER_NOT_FOUND, s::LOCATE_FOLDER)),
            },
        );
        if r.removed {
            remove = Some(i);
        }
        if r.locate {
            state_picks.push(PickPurpose::RelocateFolder(id, canonical.clone()));
        }
        if a != before
            && let Some(f) = e.draft.folders.get_mut(i)
        {
            f.access = if a == 1 {
                FolderAccess::ReadWrite
            } else {
                FolderAccess::ReadOnly
            };
            changed = true;
        }
        for issue in issues
            .iter()
            .filter(|x| x.path.as_deref() == Some(canonical.as_path()))
        {
            issue_line(
                ui,
                issue_text(&issue.message),
                issue.severity >= IssueSeverity::Error,
            );
        }
        if a == 1 && network == NetworkAccess::Full {
            issue_line(ui, s::WRITABLE_WITH_INTERNET, false);
        }
    }
    if let Some(i) = remove {
        e.draft.folders.remove(i);
        changed = true;
    }
    state.picks.extend(state_picks);
    ui.horizontal(|ui| {
        if ui
            .add(
                Button::new(s::ADD_FOLDER)
                    .icon(icons::FOLDER_PLUS)
                    .size(Size::Sm),
            )
            .clicked()
        {
            state.picks.push(PickPurpose::FolderGrant(Some(id)));
        }
        ui.label(Type::Caption.rich(s::DROP_FOLDER).color(p.text_muted));
    });
    let Some(e) = editor(state) else { return };
    ui.horizontal(|ui| {
        ui.label(Type::Body.rich(s::REMOVABLE).color(p.text_muted));
        let mut r = match e.draft.removable {
            RemovableAccess::None => 0,
            RemovableAccess::ReadOnly => 1,
            RemovableAccess::ReadWrite => 2,
        };
        if segmented(
            ui,
            s::REMOVABLE,
            &mut r,
            &segs(&[s::NOT_SHARED, s::CAN_READ, s::CAN_CHANGE]),
            false,
        )
        .changed()
        {
            e.draft.removable = [
                RemovableAccess::None,
                RemovableAccess::ReadOnly,
                RemovableAccess::ReadWrite,
            ][r];
            changed = true;
        }
    });
    let rect = egui::Rect::from_min_max(
        rect_start,
        ui.cursor().min + vec2(ui.available_width(), 0.0),
    );
    state.dnd.target(rect, DropTarget::FolderList(Some(id)));
    if changed {
        if let Some(e) = editor(state) {
            e.reclassify();
            e.folder_error = None;
        }
        reassess(state, env);
    }
}

/// Show the Protection tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let Some(e) = editor(state) else {
        if state.details.detail.show_skeleton(env.mono) {
            super::skeleton_lines(ui, 6);
        }
        return;
    };
    if !e.saved.is_supported_version() {
        crate::screens::details::launch_panel::state_area(ui, state, env, g);
        issue_line(ui, s::POLICY_TOO_NEW, true);
        return;
    }
    let running = g.activity != easyplay_core::model::Activity::Idle;
    if running {
        crate::widgets::banner::banner(
            ui,
            crate::widgets::banner::Severity::Info,
            s::APPLIES_NEXT_TIME,
            None,
            false,
        );
        ui.add_space(space::S3);
    }
    ui.set_max_width(ui.available_width().min(840.0));
    // Preset.
    ui.horizontal(|ui| {
        let h = ui.label(Type::Heading.rich(s::PROTECTION).color(p.text_primary));
        crate::a11y::labelled(ui.ctx(), h.id, egui::Role::Heading, s::PROTECTION);
        if let Preset::Custom { base } = e.draft.preset {
            ui.label(
                Type::Body
                    .rich(s::custom_based_on(base))
                    .color(p.text_secondary),
            );
            if ui
                .add(Button::new(s::RESET_TO_PRESET).variant(Variant::Link))
                .clicked()
            {
                e.confirm_preset = None;
                e.draft = env.bridge.preset_policy(base, &e.draft);
                e.draft.folders.clear();
                e.draft.save_data = SaveDataLocation::Internal;
                e.reclassify();
            }
        }
    });
    let Some(e) = editor(state) else { return };
    if let Some(b) = preset_cards(ui, e.draft.preset) {
        if e.draft.preset.is_custom() {
            e.confirm_preset = Some(b);
        } else {
            e.draft = env.bridge.preset_policy(b, &e.draft);
        }
    }
    if let Some(b) = e.confirm_preset {
        ui.horizontal_wrapped(|ui| {
            issue_line(ui, &s::replace_custom(b), false);
            if ui.add(Button::new(s::REPLACE).size(Size::Sm)).clicked() {
                e.draft = env.bridge.preset_policy(b, &e.draft);
                e.confirm_preset = None;
            }
            if ui
                .add(Button::ghost(strings::dialogs::CANCEL).size(Size::Sm))
                .clicked()
            {
                e.confirm_preset = None;
            }
        });
    }
    reassess(state, env);
    let Some(e) = editor(state) else { return };
    ui.add_space(space::S3);
    // Summary with markers.
    let statements = e.assessment.statements.clone();
    let removed: Vec<String> = e.removed().into_iter().map(str::to_owned).collect();
    let markers: Vec<bool> = statements.iter().map(|st| e.is_added(&st.text)).collect();
    card(ui, s::REACH, |ui| {
        for (st, added) in statements.iter().zip(markers) {
            statement_row(ui, st, added.then_some(s::ADDED_MARK));
        }
        for r in &removed {
            ui.horizontal(|ui| {
                ui.label(icons::icon(icons::CLOSE, icon_size::INLINE).color(p.text_muted));
                ui.label(Type::Body.rich(r).color(p.text_muted).strikethrough());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(Type::Caption.rich(s::REMOVED_MARK).color(p.text_muted));
                });
            });
        }
    });
    // Internet.
    let Some(e) = editor(state) else { return };
    section(ui, s::INTERNET, None);
    let mut net = usize::from(e.draft.network == NetworkAccess::Full);
    if segmented(
        ui,
        s::INTERNET,
        &mut net,
        &segs(&[s::NO_INTERNET, s::INTERNET_ON]),
        false,
    )
    .changed()
    {
        e.draft.network = if net == 1 {
            NetworkAccess::Full
        } else {
            NetworkAccess::None
        };
        e.reclassify();
    }
    folders_section(ui, state, env, g);
    // Save data.
    let Some(e) = editor(state) else { return };
    section(ui, s::SAVE_DATA, None);
    let mut sd = usize::from(matches!(
        e.draft.save_data,
        SaveDataLocation::External { .. }
    ));
    if segmented(
        ui,
        s::SAVE_DATA,
        &mut sd,
        &segs(&[s::PRIVATE_STORAGE, s::CHOOSE_FOLDER]),
        false,
    )
    .changed()
    {
        if sd == 0 {
            e.draft.save_data = SaveDataLocation::Internal;
            e.reclassify();
        } else {
            state.picks.push(PickPurpose::SaveFolder(Some(g.id)));
        }
    }
    let Some(e) = editor(state) else { return };
    if let SaveDataLocation::External { dir } = &e.draft.save_data {
        ui.label(
            Type::Mono
                .rich(dir.display().to_string())
                .color(p.text_secondary),
        );
    }
    // Devices.
    section(ui, s::DEVICES, None);
    let mut sound = e.draft.devices.audio == AudioAccess::Full;
    if toggle(ui, &mut sound, s::SOUND, None).changed() {
        e.draft.devices.audio = if sound {
            AudioAccess::Full
        } else {
            AudioAccess::None
        };
        e.reclassify();
    }
    let input_readable = env.bridge.host().input_readable;
    ui.horizontal(|ui| {
        ui.label(Type::Body.rich(s::CONTROLLERS).color(p.text_primary));
        let mut c = match e.draft.devices.controllers {
            ControllerAccess::None => 0,
            ControllerAccess::Detected => 1,
            ControllerAccess::AllInput => 2,
        };
        let r = segmented(
            ui,
            s::CONTROLLERS,
            &mut c,
            &segs(&[s::CTRL_NONE, s::CTRL_CONNECTED, s::CTRL_ALL]),
            false,
        );
        if r.changed() {
            if c == 2 && input_readable {
                c = 1;
            }
            e.draft.devices.controllers = [
                ControllerAccess::None,
                ControllerAccess::Detected,
                ControllerAccess::AllInput,
            ][c];
            e.reclassify();
        }
    });
    if input_readable {
        ui.label(
            Type::Caption
                .rich(s::ALL_INPUT_UNAVAILABLE)
                .color(p.text_muted),
        );
    }
    let mut cam = e.draft.devices.camera;
    if toggle(ui, &mut cam, s::CAMERA, None).changed() {
        e.draft.devices.camera = cam;
        e.reclassify();
    }
    if env.bridge.host().x11 {
        section(ui, s::DISPLAY, None);
        issue_line(ui, s::X11_NOTICE, false);
    }
    // Advanced.
    ui.add_space(space::S4);
    let n = s::advanced_changes(&e.saved, &e.draft);
    if ui
        .add(Button::ghost(&s::advanced(n)).icon(if e.advanced {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        }))
        .clicked()
    {
        e.advanced = !e.advanced;
    }
    if e.advanced {
        egui::Frame::NONE
            .inner_margin(egui::Margin {
                left: space::S6 as i8,
                ..Default::default()
            })
            .show(ui, |ui| {
                ui.label(
                    Type::BodyStrong
                        .rich(s::GAME_FOLDER_ACCESS)
                        .color(p.text_primary),
                );
                let mut gd = match e.draft.game_dir {
                    GameDirAccess::ReadWrite => 0,
                    GameDirAccess::ReadOnly | GameDirAccess::FileOnly => 1,
                    GameDirAccess::Protected => 2,
                };
                if g.root.is_none() {
                    ui.label(
                        Type::Caption
                            .rich(s::FILE_ONLY_EXPLAINED)
                            .color(p.text_muted),
                    );
                } else if segmented(
                    ui,
                    s::GAME_FOLDER_ACCESS,
                    &mut gd,
                    &segs(&[s::CAN_CHANGE, s::READ_ONLY, s::PROTECTED]),
                    false,
                )
                .changed()
                {
                    e.draft.game_dir = [
                        GameDirAccess::ReadWrite,
                        GameDirAccess::ReadOnly,
                        GameDirAccess::Protected,
                    ][gd];
                    e.reclassify();
                }
                ui.add_space(space::S2);
                ui.label(Type::BodyStrong.rich(s::IDENTITY).color(p.text_primary));
                let h = &mut e.draft.hardening;
                let mut changed = false;
                changed |= toggle(
                    ui,
                    &mut h.mask_hostname,
                    s::MASK_HOSTNAME,
                    Some(s::MASK_HOSTNAME_BREAKS),
                )
                .changed();
                changed |= toggle(
                    ui,
                    &mut h.mask_machine_id,
                    s::MASK_MACHINE_ID,
                    Some(s::MASK_MACHINE_ID_BREAKS),
                )
                .changed();
                changed |= toggle(
                    ui,
                    &mut h.mask_hardware_ids,
                    s::MASK_HARDWARE,
                    Some(s::MASK_HARDWARE_BREAKS),
                )
                .changed();
                let mut filter = h.seccomp == SeccompMode::Default;
                if toggle(ui, &mut filter, s::SECCOMP, Some(s::SECCOMP_HINT)).changed() {
                    h.seccomp = if filter {
                        SeccompMode::Default
                    } else {
                        SeccompMode::Off
                    };
                    changed = true;
                }
                ui.label(
                    Type::BodyStrong
                        .rich(s::LAYER_ISOLATION)
                        .color(p.text_primary),
                );
                let mut iso = match h.runtime_isolation {
                    RuntimeIsolation::Auto => 0,
                    RuntimeIsolation::Isolated => 1,
                    RuntimeIsolation::Shared => 2,
                };
                if segmented(
                    ui,
                    s::LAYER_ISOLATION,
                    &mut iso,
                    &segs(&[s::ISO_AUTO, s::ISO_PRIVATE, s::ISO_SHARED]),
                    false,
                )
                .changed()
                {
                    h.runtime_isolation = [
                        RuntimeIsolation::Auto,
                        RuntimeIsolation::Isolated,
                        RuntimeIsolation::Shared,
                    ][iso];
                    changed = true;
                }
                changed |= toggle(
                    ui,
                    &mut e.draft.devices.alsa_devices,
                    s::DIRECT_SOUND,
                    Some(s::DIRECT_SOUND_HINT),
                )
                .changed();
                if changed {
                    e.reclassify();
                }
            });
    }
    let _ = stroke::BORDER;
}
