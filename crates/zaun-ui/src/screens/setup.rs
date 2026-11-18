//! "Set up this game" (script `addGameDialog`, step 3; SCREEN_SPECS §6, UI-015, UF-2): the
//! last step of the add-a-game dialog ([`super::add_game::dialog_id`]), and the dialog a
//! program opened from the file manager arrives in.
//!
//! The design's part: the cover preview (112) at the left, which is the poster composing
//! itself as the name is typed; beside it the Name field, the Protection segmented control
//! (Offline / Online / Standard; "Custom (based on …)" with *Reset to preset* while the
//! switches differ), and the *Can use* / *Can't use* lists; under them the fine print about
//! what protection can do, with *Learn more*. Actions: *Back* (or *Cancel*) at the left,
//! *Add to library* and *Play* (*Run installer* for an installer) at the right.
//!
//! The app's part, in the design's parts: hints and problems of the draft as notes, *I
//! understand* when a warning needs it, and *More options*: the game folder, shared folders,
//! game-folder access, save data, devices, the compatibility layer, arguments and the
//! entry's type as setting rows. After *Play* the dialog shows the launch until the game
//! runs (*Run in background* closes it).

use egui::text::{CCursor, CCursorRange};
use egui::{
    Align, Context, Id, Key, Layout, Modifiers, Popup, Rect, Sense, Ui, UiBuilder, pos2, vec2,
};
use zaun_core::model::{
    AudioAccess, BasePreset, ControllerAccess, EntryKind, GameDirAccess, GrantOrigin,
    SaveDataLocation,
};

use super::Env;
use super::add_game::{dialog_id, pending_id, stash_id};
use super::details::launch_panel;
use super::details::permissions::{USER_DIRS, issue_text};
use crate::app::UiState;
use crate::bridge::{AfterCreate, Command, UiCommand};
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::route::Overlay;
use crate::strings;
use crate::strings::setup as s;
use crate::theme::tokens::{layout, overlay};
use crate::theme::{self, Type, icons};
use crate::vm::screens::AddGameVm;
use crate::vm::setup::{SetupPhase, SetupVm};
use crate::widgets::banner::{Note, Severity};
use crate::widgets::button::{Button, Variant};
use crate::widgets::choice::checkbox;
use crate::widgets::context_menu;
use crate::widgets::cover::{self, CoverLook, CoverSource};
use crate::widgets::modal::{Spec, closed, fine_print, opening, show as dialog};
use crate::widgets::rows::{SettingRow, fact_text, heading, reach};
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::text_field::TextField;
use crate::widgets::toggle::switch;

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

fn segs<'a>(labels: &[&'a str]) -> Vec<Segment<'a>> {
    labels
        .iter()
        .map(|l| Segment {
            icon: None,
            label: l,
        })
        .collect()
}

/// A quiet 12 pt label over a control ("Name", "Protection").
fn small_label(ui: &mut Ui, text: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    // 12 pt on the 18 pt line of the text around it.
    let g = Type::Caption.line(ui, text, p.t3);
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), Type::Body.line_height()),
        Sense::hover(),
    );
    ui.painter().galley(
        pos2(rect.left(), rect.center().y - g.size().y / 2.0),
        g,
        p.t3,
    );
    ui.add_space(overlay::KEYS_GAP);
}

/// The column beside the cover: name, protection, reach.
fn identity(ui: &mut Ui, v: &mut SetupVm, env: &Env<'_>, focus_name: bool) -> Id {
    let p = theme::theme(ui.ctx()).palette.clone();
    small_label(ui, s::NAME());
    let err = v.name_error().map(|_| s::NAME_REQUIRED());
    let f = TextField::new(&mut v.name, "setup.name")
        .accessible_name(s::NAME())
        .error(err)
        .width(ui.available_width())
        .show(ui);
    if focus_name {
        // The name has focus and is selected, so typing replaces it.
        f.response.request_focus();
        let id = f.response.id;
        let len = v.name.chars().count();
        if let Some(mut st) = egui::text_edit::TextEditState::load(ui.ctx(), id) {
            st.cursor
                .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
            st.store(ui.ctx(), id);
        }
    }
    ui.add_space(overlay::BLANK_FINE_GAP);
    small_label(ui, s::PROTECTION());
    let customised = v.customised();
    let mut idx = if customised {
        BasePreset::ALL.len()
    } else {
        BasePreset::ALL
            .iter()
            .position(|b| *b == v.base())
            .unwrap_or(0)
    };
    let names: Vec<&str> = BasePreset::ALL
        .iter()
        .map(|b| strings::shield::preset_name((*b).into()))
        .collect();
    if segmented(ui, s::PROTECTION(), &mut idx, &segs(&names), false).changed()
        && let Some(b) = BasePreset::ALL.get(idx)
    {
        set_preset(v, env, *b);
    }
    if customised {
        ui.add_space(overlay::BLANK_TEXT_GAP);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = overlay::KEYS_GAP;
            ui.label(
                Type::Body
                    .rich(strings::details::custom_based_on(v.base()))
                    .color(p.t3),
            );
            if ui
                .add(Button::new(strings::details::RESET_TO_PRESET()).variant(Variant::Link))
                .clicked()
            {
                let b = v.base();
                v.draft.policy = env.bridge.preset_policy(b, &v.original.policy);
                reassess(v, env);
            }
        });
    }
    ui.add_space(overlay::DIALOG_ACTION_GAP + overlay::KEYS_GAP);
    let (can, cant) = crate::vm::details::reach(&v.draft.policy);
    reach(
        ui,
        strings::details::CAN_USE(),
        &can,
        strings::details::CANT_USE(),
        &cant,
    );
    f.response.id
}

/// The fine print about protection, ending in *Learn more*, which explains in a popover.
fn honesty(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width();
    let fmt = |t: Type, c| egui::TextFormat {
        line_height: Some(overlay::FINE_LINE),
        ..t.format(c, false)
    };
    let mut job = egui::text::LayoutJob::default();
    // Right-to-left text is broken into lines and ordered by `wrap_text`; the link then
    // follows on a line of its own.
    let lead = if crate::i18n::bidi::has_rtl(s::HONESTY()) {
        format!("{}\n", Type::Caption.wrap_text(ui, s::HONESTY(), w))
    } else {
        format!("{} ", s::HONESTY())
    };
    job.append(&lead, 0.0, fmt(Type::Caption, p.t3));
    job.append(s::LEARN_MORE(), 0.0, fmt(Type::Label, p.accent_text));
    job.wrap.max_width = w;
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let (rect, _) = ui.allocate_exact_size(g.size(), Sense::hover());
    let start = lead.chars().count();
    let a = g.pos_from_cursor(CCursor::new(start));
    let b = g.pos_from_cursor(CCursor::new(start + s::LEARN_MORE().chars().count()));
    let link = Rect::from_min_max(a.min, pos2(b.max.x.max(a.max.x), a.max.y))
        .translate(rect.min.to_vec2());
    ui.painter().galley(rect.min, g, p.t3);
    let r = ui.interact(link, ui.scope_id().with("learn-more"), Sense::click());
    crate::a11y::button(&r, s::LEARN_MORE());
    if r.hovered() {
        ui.painter().hline(
            link.x_range(),
            link.bottom() - theme::tokens::space::S0,
            egui::Stroke::new(theme::tokens::stroke::BORDER, p.accent_text),
        );
    }
    if r.has_focus() {
        crate::widgets::focus_ring(
            ui.painter(),
            link,
            egui::CornerRadius::same(theme::tokens::radius::XS),
            &theme::theme(ui.ctx()),
        );
    }
    context_menu::popup(Popup::menu(&r).width(layout::MENU_W[1]), |ui| {
        let [pad, _] = overlay::MENU_ITEM_PAD;
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(pad as i8))
            .show(ui, |ui| {
                Type::Body.paragraph(ui, s::HONESTY_MORE(), p.t2);
            });
    });
}

fn folder_label(v: &SetupVm, home: &std::path::Path) -> String {
    match &v.draft.root {
        Some(r) => s::breadcrumb(r, home),
        None => s::FILE_ONLY().to_owned(),
    }
}

/// *More options*: everything the draft holds besides name and level, as setting rows.
fn more_options(ui: &mut Ui, v: &mut SetupVm, picks: &mut Vec<PickPurpose>, env: &mut Env<'_>) {
    let mut changed = false;
    // Where the game lives.
    ui.add_space(overlay::KEYS_GAP);
    let home = env.bridge.host().home.clone();
    let label = folder_label(v, &home);
    let mut choose = None;
    SettingRow::new(s::GAME_FOLDER()).show(ui, |ui| {
        let r = ui.add(
            Button::new(&label)
                .caret()
                .accessible_name(s::folder_button(&label)),
        );
        context_menu::popup(Popup::menu(&r), |ui| {
            for c in v.original.proposal.candidates.clone() {
                let l = s::breadcrumb(&c, &home);
                if context_menu::option_item(ui, &l, v.draft.root.as_ref() == Some(&c)).clicked() {
                    choose = Some(Some(c));
                }
            }
            if context_menu::option_item(ui, s::FILE_ONLY(), v.draft.root.is_none()).clicked() {
                choose = Some(None);
            }
            context_menu::separator(ui);
            if context_menu::item(ui, None, s::CHOOSE_FOLDER(), None, false).clicked() {
                picks.push(PickPurpose::SetupRoot);
            }
        });
    });
    if let Some(c) = choose {
        v.set_root(c);
        reassess(v, env);
    }
    if v.draft.root.is_some() {
        let mut gd = match v.draft.policy.game_dir {
            GameDirAccess::ReadWrite => 0,
            GameDirAccess::ReadOnly | GameDirAccess::FileOnly => 1,
            GameDirAccess::Protected => 2,
        };
        let opts = segs(&[
            strings::details::CAN_CHANGE(),
            strings::details::READ_ONLY(),
            strings::details::PROTECTED(),
        ]);
        let moved = SettingRow::new(strings::details::GAME_FOLDER_ACCESS()).show(ui, |ui| {
            segmented(
                ui,
                strings::details::GAME_FOLDER_ACCESS(),
                &mut gd,
                &opts,
                false,
            )
            .changed()
        });
        if moved {
            v.draft.policy.game_dir = [
                GameDirAccess::ReadWrite,
                GameDirAccess::ReadOnly,
                GameDirAccess::Protected,
            ][gd];
            changed = true;
        }
    }
    let mut sd = usize::from(matches!(
        v.draft.policy.save_data,
        SaveDataLocation::External { .. }
    ));
    let opts = segs(&[
        strings::details::PRIVATE_STORAGE(),
        strings::details::CHOOSE_FOLDER(),
    ]);
    let moved = SettingRow::new(strings::details::SAVE_DATA()).show(ui, |ui| {
        segmented(ui, strings::details::SAVE_DATA(), &mut sd, &opts, false).changed()
    });
    if moved {
        if sd == 0 {
            v.draft.policy.save_data = SaveDataLocation::Internal;
            changed = true;
        } else {
            picks.push(PickPurpose::SaveFolder(None));
        }
    }
    // Shared folders.
    heading(ui, s::FILES(), false);
    for d in USER_DIRS {
        let mut on = v
            .draft
            .policy
            .folders
            .iter()
            .any(|f| f.origin == GrantOrigin::WellKnown(d));
        let name = crate::demo::user_dir_name(d);
        let flipped = SettingRow::new(name).show(ui, |ui| {
            switch(ui, &mut on, &strings::details::share_toggle(name)).changed()
        });
        if flipped && let Some(path) = super::details::permissions::user_dir_path(env, d) {
            let mut e =
                crate::vm::details::PolicyEditor::new(v.draft.policy.clone(), v.assessment.clone());
            e.toggle_user_dir(d, path, env.now);
            v.draft.policy = e.draft;
            changed = true;
        }
    }
    for f in v
        .draft
        .policy
        .folders
        .iter()
        .filter(|f| f.origin == GrantOrigin::UserChosen)
    {
        SettingRow::new(&f.path.display().to_string())
            .mono()
            .show(ui, |_| ());
    }
    if let Some(e) = &v.folder_error {
        let text = strings::details::rejected(e);
        Note::new(Severity::Danger, &text).show(ui);
    }
    ui.add_space(overlay::KEYS_GAP);
    if ui
        .add(Button::new(strings::details::ADD_FOLDER()))
        .clicked()
    {
        picks.push(PickPurpose::FolderGrant(None));
    }
    // Devices.
    heading(ui, strings::details::DEVICES(), false);
    let audio = v.draft.policy.devices.audio;
    let mut sound = audio != AudioAccess::None;
    let mut microphone = audio == AudioAccess::Full;
    let sound_changed = SettingRow::new(strings::details::SOUND()).show(ui, |ui| {
        switch(ui, &mut sound, strings::details::SOUND()).changed()
    });
    let microphone_changed = SettingRow::new(strings::details::MICROPHONE()).show(ui, |ui| {
        switch(ui, &mut microphone, strings::details::MICROPHONE()).changed()
    });
    if sound_changed || microphone_changed {
        // The microphone needs sound; sound switched off takes the microphone with it.
        v.draft.policy.devices.audio = if microphone_changed && microphone {
            AudioAccess::Full
        } else if !sound {
            AudioAccess::None
        } else if microphone {
            AudioAccess::Full
        } else {
            AudioAccess::Playback
        };
        changed = true;
    }
    let mut ctrl = v.draft.policy.devices.controllers != ControllerAccess::None;
    if SettingRow::new(strings::details::CONTROLLERS()).show(ui, |ui| {
        switch(ui, &mut ctrl, strings::details::CONTROLLERS()).changed()
    }) {
        v.draft.policy.devices.controllers = if ctrl {
            ControllerAccess::Detected
        } else {
            ControllerAccess::None
        };
        changed = true;
    }
    let mut camera = v.draft.policy.devices.camera;
    if SettingRow::new(strings::details::CAMERA()).show(ui, |ui| {
        switch(ui, &mut camera, strings::details::CAMERA()).changed()
    }) {
        v.draft.policy.devices.camera = camera;
        changed = true;
    }
    // How it starts.
    heading(ui, s::LAUNCH(), false);
    fact_text(
        ui,
        strings::details::LAYER(),
        &super::details::runtime::runtime_label(&v.draft.runtime, env),
    );
    let args_err = zaun_core::model::split_args(&v.args)
        .err()
        .map(|_| strings::details::ARGS_UNBALANCED());
    ui.add_space(overlay::KEYS_GAP);
    small_label(ui, strings::details::ARGUMENTS());
    TextField::new(&mut v.args, "setup.args")
        .accessible_name(strings::details::ARGUMENTS())
        .mono()
        .error(args_err)
        .width(ui.available_width())
        .show(ui);
    let mut k = match v.draft.kind {
        EntryKind::Game => 0,
        EntryKind::Installer => 1,
        EntryKind::Tool => 2,
    };
    let opts = segs(&[s::KIND_GAME(), s::KIND_INSTALLER(), s::KIND_TOOL()]);
    let moved = SettingRow::new(s::TYPE()).show(ui, |ui| {
        segmented(ui, s::TYPE(), &mut k, &opts, false).changed()
    });
    if moved {
        v.draft.kind = [EntryKind::Game, EntryKind::Installer, EntryKind::Tool][k];
    }
    if changed {
        v.reclassify();
        reassess(v, env);
    }
}

/// Returns the name field's id.
fn editing_body(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    focus_name: bool,
) -> Option<Id> {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let mut picks = Vec::new();
    let v = state.setup.as_mut()?;
    let body_top = ui.cursor().min;
    let (i, n) = v.context.queue;
    if n > 1 {
        fine_print(ui, &s::queue(i, n));
        ui.add_space(overlay::PRE[0]);
    }
    // The cover preview at the left is the poster of the name as it is typed.
    let w = ui.available_width();
    let top = ui.cursor().min;
    let cw = layout::DIALOG_COVER;
    let cover_rect = Rect::from_min_size(top, vec2(cw, cw * layout::COVER_ASPECT));
    let title = if v.name.trim().is_empty() {
        s::UNTITLED()
    } else {
        v.name.trim()
    };
    cover::paint(
        ui.painter(),
        cover_rect,
        CoverSource::Placeholder,
        title,
        CoverLook::default(),
        &theme,
    );
    let preview = ui.interact(
        cover_rect,
        ui.make_persistent_id("setup.cover"),
        Sense::hover(),
    );
    crate::a11y::item(&preview, egui::Role::Image, &s::cover_preview(title));
    let x = top.x + cw + overlay::DIALOG_COVER_GAP;
    let mut col = ui.new_child(
        UiBuilder::new()
            .id_salt("setup.identity")
            .max_rect(Rect::from_min_size(
                pos2(x, top.y),
                vec2(w - cw - overlay::DIALOG_COVER_GAP, f32::INFINITY),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    col.spacing_mut().item_spacing.y = 0.0;
    let name_id = identity(&mut col, v, env, focus_name);
    let bottom = col.min_rect().bottom().max(cover_rect.bottom());
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + w, bottom)),
        Sense::hover(),
    );
    ui.add_space(overlay::BLANK_FINE_GAP);
    honesty(ui);
    // Hints and problems of the draft.
    let mut notes: Vec<(Severity, String)> = Vec::new();
    if v.draft.root.is_none() {
        notes.push((Severity::Info, s::LOOSE_FILE().to_owned()));
    }
    if v.is_installer() {
        notes.push((Severity::Info, s::INSTALLER_HINT().to_owned()));
    }
    if v.draft.hints.online_capable && v.base() == BasePreset::Offline {
        notes.push((Severity::Info, s::ONLINE_HINT().to_owned()));
    }
    for issue in &v.assessment.issues {
        use zaun_core::sandbox::IssueSeverity;
        if issue.severity >= IssueSeverity::Warning {
            let sev = if issue.severity >= IssueSeverity::Error {
                Severity::Danger
            } else {
                Severity::Warning
            };
            notes.push((sev, issue_text(&issue.message).to_owned()));
        }
    }
    for (sev, text) in &notes {
        ui.add_space(overlay::BLANK_TEXT_GAP);
        Note::new(*sev, text).show(ui);
    }
    if v.warnings() > 0 {
        ui.add_space(overlay::KEYS_GAP);
        checkbox(ui, &mut v.acknowledged, s::I_UNDERSTAND());
    }
    if !v.can_continue() {
        ui.add_space(overlay::KEYS_GAP);
        ui.label(Type::Caption.rich(s::FIX_TO_CONTINUE()).color(p.coral));
    }
    ui.add_space(overlay::KEYS_GAP);
    let caret = if v.more {
        icons::CARET_DOWN
    } else {
        icons::CARET_RIGHT
    };
    if ui
        .add(Button::ghost(s::MORE_OPTIONS()).icon(caret))
        .clicked()
    {
        v.more = !v.more;
    }
    if v.more {
        more_options(ui, v, &mut picks, env);
    }
    let r = Rect::from_min_max(body_top, ui.min_rect().max);
    state.dnd.target(r, DropTarget::FolderList(None));
    state.picks.extend(picks);
    Some(name_id)
}

fn launching_body(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let Some(v) = state.setup.as_ref() else {
        return;
    };
    let SetupPhase::Launching { game } = v.phase.clone() else {
        return;
    };
    let name = v.name.clone();
    let top = ui.cursor().min;
    let cw = layout::DIALOG_COVER;
    let cover_rect = Rect::from_min_size(top, vec2(cw, cw * layout::COVER_ASPECT));
    cover::paint(
        ui.painter(),
        cover_rect,
        CoverSource::Placeholder,
        &name,
        CoverLook::default(),
        &theme,
    );
    let w = ui.available_width();
    let mut col = ui.new_child(
        UiBuilder::new()
            .id_salt("setup.launching")
            .max_rect(Rect::from_min_size(
                pos2(top.x + cw + overlay::DIALOG_COVER_GAP, top.y),
                vec2(w - cw - overlay::DIALOG_COVER_GAP, f32::INFINITY),
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    match game {
        None => {
            col.label(Type::Body.rich(strings::launch::CHECKING()).color(p.t2));
        }
        Some(g) => {
            if let Some(l) = state.launches.active(g).cloned() {
                if launch_panel::progress_row(&mut col, &l, env.mono) {
                    env.bridge.send(Command::CancelLaunch { id: g });
                }
            } else if state.launches.failures.contains_key(&g) {
                let err = state.launches.failures.get(&g).cloned();
                launch_panel::problem_panel(&mut col, state, env, g, err);
            } else if state
                .launches
                .games
                .get(&g)
                .is_some_and(|l| matches!(l.state, zaun_core::launch::LaunchState::Running { .. }))
            {
                col.label(Type::Body.rich(strings::launch::RUNNING()).color(p.t2));
            }
        }
    }
    let bottom = col.min_rect().bottom().max(cover_rect.bottom());
    ui.allocate_rect(
        Rect::from_min_max(top, pos2(top.x + w, bottom)),
        Sense::hover(),
    );
}

/// What the actions row asked for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Foot {
    None,
    Back,
    Add,
    Play,
    ApplyRest,
    Background,
}

/// Show the dialog.
pub fn show(ctx: &Context, _id: Id, state: &mut UiState, env: &mut Env<'_>) {
    let id = dialog_id();
    let Some(v) = state.setup.as_ref() else {
        state.nav.close(Overlay::Setup);
        return;
    };
    let editing = v.phase == SetupPhase::Editing;
    let unsaved = editing && v.draft != v.original;
    // Alt+1/2/3 choose the level.
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
    // The launch view closes on its own once the game runs, and when its launch is over
    // with nothing to show here (cancelled, or the game already ended); a failure stays.
    if let Some(SetupPhase::Launching { game: Some(g) }) =
        state.setup.as_ref().map(|v| v.phase.clone())
        && state.launches.games.get(&g).is_some_and(|l| {
            use zaun_core::launch::LaunchState;
            matches!(
                l.state,
                LaunchState::Running { .. } | LaunchState::Cancelled | LaunchState::Finished { .. }
            )
        })
    {
        finish(ctx, id, state, true);
        return;
    }
    let title = if editing { s::TITLE } else { s::STARTING_TITLE };
    let (ok, plays, installer, batch) =
        state
            .setup
            .as_ref()
            .map_or((false, false, false, false), |v| {
                (
                    v.can_continue(),
                    v.primary_plays(),
                    v.is_installer(),
                    v.context.queue.1 > 1,
                )
            });
    // The setup Continue waited for has arrived: nothing is awaited any more.
    ctx.data_mut(|d| d.remove_temp::<f64>(pending_id()));
    let stash: Option<AddGameVm> = ctx.data(|d| d.get_temp(stash_id()));
    // The name takes focus when the step shows (and when the setup asks for it).
    let first = opening(ctx, id.with("setup"));
    let focus_name = editing
        && (first
            || state
                .setup
                .as_mut()
                .is_some_and(|v| std::mem::take(&mut v.focus_primary)));
    let r = dialog(
        ctx,
        id,
        Spec::new(title())
            .wide()
            .close_button()
            .unsaved(unsaved)
            .step(if editing { 2 } else { 3 }),
        |ui| {
            if editing {
                editing_body(ui, state, env, focus_name)
            } else {
                launching_body(ui, state, env);
                None
            }
        },
        |ui| {
            let mut out = Foot::None;
            if !editing {
                if ui.add(Button::new(s::RUN_IN_BACKGROUND())).clicked() {
                    out = Foot::Background;
                }
                return out;
            }
            let reason = (!ok).then_some(s::FIX_TO_CONTINUE());
            let main_label = if installer {
                strings::play::RUN_INSTALLER()
            } else {
                strings::play::PLAY()
            };
            if ui
                .add(
                    Button::primary(main_label)
                        .icon(icons::PLAY)
                        .disabled(reason),
                )
                .clicked()
            {
                out = Foot::Play;
            }
            if ui
                .add(Button::new(s::ADD_TO_LIBRARY()).disabled(reason))
                .clicked()
            {
                out = Foot::Add;
            }
            if batch
                && ui
                    .add(Button::ghost(s::USE_FOR_REST()).disabled(reason))
                    .clicked()
            {
                out = Foot::ApplyRest;
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                // Back to the programs of the folder; Cancel when there is no step before,
                // or several programs are being set up one after another.
                let label = if stash.is_some() && !batch {
                    s::BACK
                } else {
                    s::CANCEL
                };
                if ui.add(Button::new(label())).clicked() {
                    out = Foot::Back;
                }
            });
            out
        },
    );
    // Enter in the name field (or with nothing focused) completes the common case: Play
    // from the file manager, Add to library from inside Zaun.
    let mut foot = r.footer;
    // What had focus in the pass before: Enter that ends another field's editing (it leaves
    // focus empty) must not complete the dialog.
    let focused = ctx.memory(|m| m.focused());
    let before_key = id.with("setup.focus_before");
    let before: Option<Id> = ctx.data(|d| d.get_temp::<Option<Id>>(before_key)).flatten();
    ctx.data_mut(|d| d.insert_temp(before_key, focused));
    if editing && foot == Foot::None && ok && !r.close_requested {
        let name = r.body;
        let in_name = focused.is_some() && focused == name
            || focused.is_none() && (before.is_none() || before == name);
        if in_name && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
            foot = if plays || installer {
                Foot::Play
            } else {
                Foot::Add
            };
        }
    }
    match foot {
        Foot::Play | Foot::Add | Foot::ApplyRest => {
            let Some(v) = state.setup.as_mut() else {
                return;
            };
            let then = match foot {
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
                finish(ctx, id, state, true);
            }
        }
        Foot::Background => finish(ctx, id, state, true),
        Foot::Back => {
            env.bridge.send(Command::DiscardDraft);
            let stash = stash.filter(|_| !batch);
            finish(ctx, id, state, stash.is_none());
            if let Some(a) = stash {
                // Back to the programs found in the folder: the same dialog, a step back.
                state.add_game = Some(a);
                state.nav.open(Overlay::AddGame);
            }
        }
        Foot::None => {
            if r.close_requested {
                if editing {
                    env.bridge.send(Command::DiscardDraft);
                }
                ctx.data_mut(|d| d.remove_temp::<AddGameVm>(stash_id()));
                state.setup = None;
                state.nav.close(Overlay::Setup);
            }
        }
    }
}

/// Close the setup; `done` when the whole dialog ends (focus returns to its invoker).
fn finish(ctx: &Context, id: Id, state: &mut UiState, done: bool) {
    if done {
        closed(ctx, id);
        ctx.data_mut(|d| d.remove_temp::<AddGameVm>(stash_id()));
    }
    state.setup = None;
    state.nav.close(Overlay::Setup);
}
