//! Protection tab (easyplay-redesign.html `tabProtection`, `levelHTML`; SCREEN_SPECS_2 §8–9,
//! ADR-0023). The level as a segmented control (nothing chosen while the game is
//! customised, with "Custom, based on Offline. Reset to Offline"), what the game can and
//! can't use as two short lists rebuilt as switches flip, then the switches in two columns:
//! *Internet and devices* and *Files and folders* (one column when the page's column is
//! narrow). Every row is a setting row with its control at the right; a permission that
//! comes in degrees shows a small picker under its label while it is on. Beneath: what
//! Advanced holds, and the save bar (in the page). A rejected folder never enters the list:
//! the message names the rule instead.

use std::path::PathBuf;

use easyplay_core::ids::GameId;
use easyplay_core::model::{
    BasePreset, FolderAccess, FolderGrant, GameDirAccess, GameSummary, GrantOrigin, Permissions,
    RuntimeIsolation, SaveDataLocation, UserDir,
};
use easyplay_core::sandbox::{AccessCategory, IssueCode, IssueSeverity, StatementSeverity};
use egui::{Align, Color32, Layout, Rect, Ui, UiBuilder, pos2, vec2};

use super::{Note, NoteTone, note};
use crate::app::{UiState, assess_draft};
use crate::bridge::{GameEdit, UiCommand};
use crate::demo::{grant_name, user_dir_name};
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::screens::Env;
use crate::strings;
use crate::strings::details as s;
use crate::theme::tokens::{game as gm, icon as icon_size, layout, space, stroke};
use crate::theme::{self, Type, icons};
use crate::vm::details::{PolicyEditor, reach};
use crate::widgets::button::{Button, Size};
use crate::widgets::rows::{self, SettingRow};
use crate::widgets::segmented::{Segment, segmented, segmented_with_hints};
use crate::widgets::select::{select, select_inline};
use crate::widgets::toggle::switch;

pub use crate::vm::details::USER_DIRS;

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

/// Section heading with an optional description (the setup sheet, Settings).
pub fn section(ui: &mut Ui, title: &str, desc: Option<&str>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.add_space(space::S4);
    let h = ui.label(Type::Heading.rich(title).color(p.t1));
    crate::a11y::labelled(ui.ctx(), h.id, egui::Role::Heading, title);
    if let Some(d) = desc {
        Type::Caption.paragraph(ui, d, p.t2);
    }
    ui.add_space(space::S1);
}

/// An inline issue line.
pub fn issue_line(ui: &mut Ui, text: &str, error: bool) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let c = if error { p.coral } else { p.amber };
    ui.horizontal_wrapped(|ui| {
        ui.label(icons::icon(icons::WARNING, icon_size::INLINE).color(c));
        Type::Body.paragraph(ui, text, c);
    });
}

/// The English fallback of an issue message (`key|text`).
pub fn issue_text(message: &str) -> &str {
    message.split_once('|').map_or(message, |(_, t)| t)
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
        strings::feedback::SAVED(),
        crate::vm::toasts::ToastKind::Success,
        None,
        env.mono,
    );
}

/// One thing a game can be allowed.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Perm {
    /// A permission that is granted or not: its bit.
    Flag(Permissions),
    /// The game's own folder.
    GameFolder,
    /// A folder that is shared: its place in the draft.
    Folder(usize),
    /// A well-known folder that is not shared, and where it is.
    UserDir(UserDir, PathBuf),
    /// A folder the saved policy shares and the draft does not: its place in the saved
    /// policy. Its row stays until the change is saved.
    Dropped(usize),
    /// Where save data is kept.
    SaveData,
}

/// What was done to a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Picked {
    /// Its switch flipped.
    Switched(bool),
    /// A degree (or a select's option) was picked.
    Degree(usize),
}

/// How heavily a line under a row's label weighs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Weight {
    Quiet,
    Warning,
    Danger,
}

/// What stands at a row's right end.
#[derive(Clone, Debug)]
enum Control {
    /// Nothing: a row that says something.
    None,
    /// A switch, on or off; with its degrees (and the one in force) while on.
    Switch {
        on: bool,
        degrees: Option<(Vec<&'static str>, usize)>,
    },
    /// A select of these options.
    Select(Vec<String>, usize),
}

/// A row before it is drawn.
struct Row {
    perm: Option<Perm>,
    key: String,
    title: String,
    help: Option<String>,
    lines: Vec<(String, Weight)>,
    control: Control,
    /// It differs from what is saved (said to assistive technology as "changed").
    changed: bool,
}

impl Row {
    fn new(perm: Option<Perm>, key: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            perm,
            key: key.into(),
            title: title.into(),
            help: None,
            lines: Vec::new(),
            control: Control::None,
            changed: false,
        }
    }

    fn changed(mut self, changed: bool) -> Self {
        self.changed = changed;
        self
    }

    fn help(mut self, text: impl Into<String>) -> Self {
        self.help = Some(text.into());
        self
    }

    fn line(mut self, text: impl Into<String>, weight: Weight) -> Self {
        self.lines.push((text.into(), weight));
        self
    }

    fn switch(mut self, on: bool) -> Self {
        self.control = Control::Switch { on, degrees: None };
        self
    }

    /// The degrees, and the one in force (shown while the switch is on).
    fn degrees(mut self, labels: &[&'static str], chosen: usize) -> Self {
        if let Control::Switch { on: true, degrees } = &mut self.control {
            *degrees = Some((labels.to_vec(), chosen));
        }
        self
    }
}

/// The degrees of a folder, and of removable drives.
fn folder_degrees() -> [&'static str; 2] {
    [s::CAN_READ(), s::CAN_READ_CHANGE()]
}
/// *Locate…*, which follows them on a folder that is gone.
const FOLDER_LOCATE: usize = 2;

/// The row of a permission that is granted or not; `degrees` names the two degrees of one
/// that has them, the narrower first.
fn flag_row(
    e: &PolicyEditor,
    bit: Permissions,
    title: &str,
    degrees: Option<[&'static str; 2]>,
) -> Row {
    let now = e.draft.permissions();
    let on = now.contains(bit);
    let all = bit | bit.degrees();
    let mut row = Row::new(Some(Perm::Flag(bit)), format!("flag-{}", bit.bits()), title)
        .switch(on)
        .changed(e.saved.permissions() & all != now & all);
    if let Some(d) = degrees {
        row = row.degrees(&d, usize::from(now.contains(bit.degrees())));
    }
    row
}

/// Whether a grant is for this well-known folder.
fn is_dir(f: &FolderGrant, d: UserDir, path: &PathBuf) -> bool {
    f.origin == GrantOrigin::WellKnown(d) || f.canonical == *path
}

/// The row of a shared folder. `own` is false for a well-known folder, whose place its name
/// says.
fn folder_row(e: &PolicyEditor, env: &Env<'_>, i: usize, f: &FolderGrant, own: bool) -> Row {
    let writable = f.access == FolderAccess::ReadWrite;
    let changed = !e
        .saved
        .folders
        .iter()
        .any(|x| x.canonical == f.canonical && x.access == f.access);
    let about: Vec<_> = e
        .assessment
        .issues
        .iter()
        .filter(|x| {
            x.path.as_deref() == Some(f.canonical.as_path())
                || x.path.as_deref() == Some(f.path.as_path())
        })
        .collect();
    let mut row = Row::new(
        Some(Perm::Folder(i)),
        f.canonical.display().to_string(),
        grant_name(f),
    )
    .switch(true)
    .changed(changed)
    .degrees(&folder_degrees(), usize::from(writable));
    if own {
        row = row.help(s::folder_location(&f.path, &env.bridge.host().home));
    }
    if about.iter().any(|x| x.code == IssueCode::PathMissing) {
        if let Control::Switch {
            degrees: Some((d, _)),
            ..
        } = &mut row.control
        {
            d.push(s::LOCATE_FOLDER());
        }
        row = row.line(s::FOLDER_NOT_FOUND(), Weight::Danger);
    }
    for x in about.iter().filter(|x| x.code != IssueCode::PathMissing) {
        let weight = if x.severity >= IssueSeverity::Error {
            Weight::Danger
        } else {
            Weight::Warning
        };
        row = row.line(issue_text(&x.message), weight);
    }
    if writable && e.draft.permissions().contains(Permissions::INTERNET) {
        row = row.line(s::WRITABLE_WITH_INTERNET(), Weight::Warning);
    }
    row
}

/// *Files and folders*, in an order that no switch changes: the game's own folder, the save
/// data, the well-known folders, the folders the user added, removable drives.
fn files(e: &PolicyEditor, env: &Env<'_>, g: &GameSummary) -> Vec<Row> {
    let host = env.bridge.host();
    let (saved, draft) = (&e.saved, &e.draft);
    let mut out = Vec::new();

    let (name, parent) = s::location(g.root.as_deref(), &g.primary_exe_path);
    let place = s::place(&name, parent.as_deref());
    if g.root.is_some() {
        let chosen = match draft.game_dir {
            GameDirAccess::ReadWrite => 0,
            GameDirAccess::ReadOnly | GameDirAccess::FileOnly => 1,
            GameDirAccess::Protected => 2,
        };
        let mut row = Row::new(Some(Perm::GameFolder), "game-folder", s::GAME_FOLDER())
            .help(place)
            .changed(saved.game_dir != draft.game_dir);
        row.control = Control::Select(
            [
                s::CAN_READ_CHANGE(),
                s::CAN_ONLY_READ(),
                s::CHANGES_KEPT_APART(),
            ]
            .map(str::to_owned)
            .to_vec(),
            chosen,
        );
        out.push(row);
    } else {
        out.push(
            Row::new(None, "game-folder", s::PROGRAM_FILE())
                .help(place)
                .line(s::FILE_ONLY_EXPLAINED(), Weight::Quiet),
        );
    }

    let external = matches!(draft.save_data, SaveDataLocation::External { .. });
    let mut options = vec![
        s::PRIVATE_STORAGE().to_owned(),
        s::CHOOSE_FOLDER().to_owned(),
    ];
    let mut row = Row::new(Some(Perm::SaveData), "save-data", s::SAVE_DATA())
        .changed(saved.save_data != draft.save_data);
    if let SaveDataLocation::External { dir } = &draft.save_data {
        // The folder is named, in place of the choice that led to it.
        options[1] = dir.file_name().map_or_else(
            || dir.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        options.push(s::CHOOSE_FOLDER().to_owned());
        row = row.help(s::folder_location(dir, &host.home));
    }
    row.control = Control::Select(options, usize::from(external));
    out.push(row);

    let dirs: Vec<(UserDir, PathBuf)> = USER_DIRS
        .into_iter()
        .filter_map(|d| user_dir_path(env, d).map(|p| (d, p)))
        .collect();
    let known = |f: &FolderGrant| dirs.iter().any(|(d, path)| is_dir(f, *d, path));
    for (d, path) in &dirs {
        // The row keeps its key (and its switch the focus) whether the folder is shared.
        let key = format!("user-dir-{d:?}");
        if let Some(i) = draft.folders.iter().position(|f| is_dir(f, *d, path)) {
            let mut row = folder_row(e, env, i, &draft.folders[i], false);
            row.key = key;
            out.push(row);
            continue;
        }
        out.push(
            Row::new(
                Some(Perm::UserDir(*d, path.clone())),
                key,
                user_dir_name(*d),
            )
            .switch(false)
            .changed(saved.folders.iter().any(|f| is_dir(f, *d, path))),
        );
    }
    // The folders the user added: the saved ones where they stood, then the new ones.
    for (j, f) in saved.folders.iter().enumerate().filter(|(_, f)| !known(f)) {
        if let Some(i) = draft
            .folders
            .iter()
            .position(|x| x.canonical == f.canonical)
        {
            out.push(folder_row(e, env, i, &draft.folders[i], true));
            continue;
        }
        out.push(
            Row::new(
                Some(Perm::Dropped(j)),
                f.canonical.display().to_string(),
                grant_name(f),
            )
            .switch(false)
            .changed(true)
            .help(s::folder_location(&f.path, &host.home)),
        );
    }
    for (i, f) in draft.folders.iter().enumerate() {
        let added = !saved.folders.iter().any(|x| x.canonical == f.canonical);
        if added && !known(f) {
            out.push(folder_row(e, env, i, f, true));
        }
    }
    out.push(flag_row(
        e,
        Permissions::REMOVABLE,
        s::REMOVABLE(),
        Some(folder_degrees()),
    ));
    out
}

/// *Internet and devices*, and what the screen means.
fn devices(e: &PolicyEditor, env: &Env<'_>) -> Vec<Row> {
    let host = env.bridge.host();
    let granted = e.draft.permissions();
    // All input devices is offered only where the account cannot read keyboards directly.
    let controller_degrees: &[&'static str] =
        if host.input_readable && !granted.contains(Permissions::ALL_INPUT) {
            &[s::CONNECTED_ONLY()]
        } else {
            &[s::CONNECTED_ONLY(), s::ALL_INPUT()]
        };
    let mut controllers = flag_row(e, Permissions::CONTROLLERS, s::GAME_CONTROLLERS(), None)
        .degrees(
            controller_degrees,
            usize::from(granted.contains(Permissions::ALL_INPUT)),
        );
    // Why the wider degree is not offered, while the narrower one is in force.
    if controller_degrees.len() == 1 && granted.contains(Permissions::CONTROLLERS) {
        controllers = controllers.line(s::ALL_INPUT_UNAVAILABLE(), Weight::Quiet);
    }
    let mut microphone = flag_row(e, Permissions::MICROPHONE, s::MICROPHONE(), None);
    if host.sound_includes_microphone
        && granted.contains(Permissions::SOUND)
        && !granted.contains(Permissions::MICROPHONE)
    {
        microphone = microphone.line(s::MICROPHONE_INSEPARABLE(), Weight::Warning);
    }
    let mut out = vec![
        flag_row(e, Permissions::INTERNET, s::INTERNET_ON(), None),
        flag_row(e, Permissions::SOUND, s::SOUND(), None),
        microphone,
        controllers,
        flag_row(e, Permissions::CAMERA, s::CAMERA(), None),
    ];
    // Where the game draws cannot be decided here; what it means is said.
    let mut screen = Row::new(None, "screen", s::SCREEN());
    let mut x11 = false;
    for st in &e.assessment.statements {
        if st.category == AccessCategory::Display {
            x11 |= st.key.contains("x11");
            let weight = match st.severity {
                StatementSeverity::Warning => Weight::Warning,
                StatementSeverity::Danger => Weight::Danger,
                _ => Weight::Quiet,
            };
            screen = screen.line(&st.text, weight);
        }
    }
    if host.x11 && !x11 {
        screen = screen.line(s::X11_NOTICE(), Weight::Warning);
    }
    if !screen.lines.is_empty() {
        out.push(screen);
    }
    out
}

/// Draw one row. Returns what was done to it.
fn row(ui: &mut Ui, r: &Row) -> Option<Picked> {
    let p = theme::theme(ui.ctx()).palette.clone();
    let colour = |w: Weight| -> Color32 {
        match w {
            Weight::Quiet => p.t3,
            Weight::Warning => p.amber,
            Weight::Danger => p.coral,
        }
    };
    let mut degree = None;
    let mut switched = None;
    let mut picked = None;
    let mut sr = SettingRow::new(&r.title).id_salt(("permission", &r.key));
    if let Some(h) = &r.help {
        sr = sr.help(h);
    }
    let lines = &r.lines;
    let control = &r.control;
    let title = r.title.as_str();
    let removable = matches!(r.perm, Some(Perm::Folder(_)));
    // What a control of the row says after its name: the help and the lines under the
    // label, and whether it differs from what is saved.
    let description = r
        .help
        .iter()
        .map(String::as_str)
        .chain(lines.iter().map(|(t, _)| t.as_str()))
        .chain(r.changed.then_some(s::CHANGED_MARK()))
        .collect::<Vec<_>>()
        .join(". ");
    let described = |resp: &egui::Response| {
        if !description.is_empty() {
            crate::a11y::describe(resp, &description);
        }
    };
    sr.show_with(
        ui,
        |ui| {
            let mut said = None;
            for (text, weight) in lines {
                let w = ui.available_width().min(layout::HELP_MAX_W);
                let g = rows::help_galley(ui, text, colour(*weight), w);
                let (rect, resp) = ui.allocate_exact_size(g.size(), egui::Sense::hover());
                ui.painter().galley(rect.min, g, colour(*weight));
                said.get_or_insert(resp);
            }
            // A row with nothing to set is read as its title, then what it says.
            if matches!(control, Control::None)
                && let Some(resp) = said
            {
                let all: Vec<&str> = lines.iter().map(|(t, _)| t.as_str()).collect();
                crate::a11y::item(&resp, egui::Role::Label, title);
                crate::a11y::describe(&resp, &all.join(" "));
            }
            if let Control::Switch {
                on: true,
                degrees: Some((labels, chosen)),
            } = control
            {
                ui.add_space(gm::PICKER_HANG[1]);
                let top = ui.cursor().min;
                let mut at = ui.new_child(
                    UiBuilder::new()
                        .max_rect(Rect::from_min_size(
                            pos2(top.x - gm::PICKER_HANG[0], top.y),
                            vec2(ui.available_width() + gm::PICKER_HANG[0], f32::INFINITY),
                        ))
                        .layout(Layout::left_to_right(Align::Min)),
                );
                let mut k = *chosen;
                let r = select_inline(&mut at, title, &mut k, labels);
                if r.changed() {
                    degree = Some(k);
                }
                described(&r);
                let used = at.min_rect();
                ui.allocate_exact_size(vec2(1.0, used.height()), egui::Sense::hover());
            }
        },
        |ui| match control {
            Control::None => {}
            Control::Switch { on, .. } => {
                let mut v = *on;
                let r = switch(ui, &mut v, title);
                if r.changed() {
                    switched = Some(v);
                }
                // Delete takes a shared folder away, as switching it off does.
                if removable
                    && *on
                    && r.has_focus()
                    && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
                {
                    switched = Some(false);
                }
                described(&r);
            }
            Control::Select(options, chosen) => {
                let refs: Vec<&str> = options.iter().map(String::as_str).collect();
                let mut k = *chosen;
                let r = select(ui, title, None, None, &mut k, &refs, false);
                if r.changed() {
                    picked = Some(k);
                }
                described(&r);
            }
        },
    );
    switched
        .map(Picked::Switched)
        .or(degree.map(Picked::Degree))
        .or(picked.map(Picked::Degree))
}

/// Put what the user did to the row of `perm` into the draft.
fn decide(state: &mut UiState, env: &mut Env<'_>, game: GameId, perm: Perm, picked: Picked) {
    let now = env.now;
    let Some(e) = editor(state) else { return };
    // A folder the saved policy shares comes back as it was saved, at its place.
    let restore = |e: &mut PolicyEditor, j: usize| {
        if let Some(f) = e.saved.folders.get(j).cloned() {
            let at = j.min(e.draft.folders.len());
            e.draft.folders.insert(at, f);
        }
    };
    let mut pick = None;
    let (was, mut granted) = (e.saved.permissions(), e.draft.permissions());
    let external = matches!(e.draft.save_data, SaveDataLocation::External { .. });
    match (perm, picked) {
        (Perm::Flag(bit), Picked::Switched(on)) => {
            granted.set(bit, on);
            // A permission with degrees comes back on at the degree that is saved; a degree
            // switched on brings the permission it widens.
            if on {
                granted |= bit.prerequisites() | (was & bit.degrees());
            }
            e.draft.set_permissions(granted);
        }
        (Perm::Flag(bit), Picked::Degree(k)) => {
            granted.set(bit.degrees(), k == 1);
            e.draft.set_permissions(granted);
        }
        (Perm::GameFolder, Picked::Degree(k)) => {
            e.draft.game_dir = [
                GameDirAccess::ReadWrite,
                GameDirAccess::ReadOnly,
                GameDirAccess::Protected,
            ][k.min(2)];
        }
        (Perm::SaveData, Picked::Degree(0)) => e.draft.save_data = SaveDataLocation::Internal,
        // The folder already chosen, chosen again.
        (Perm::SaveData, Picked::Degree(1)) if external => return,
        (Perm::SaveData, Picked::Degree(_)) => pick = Some(PickPurpose::SaveFolder(Some(game))),
        (Perm::Folder(i), Picked::Switched(false)) => {
            if i < e.draft.folders.len() {
                e.draft.folders.remove(i);
            }
        }
        (Perm::Folder(i), Picked::Degree(k)) => {
            if let Some(f) = e.draft.folders.get_mut(i) {
                match k {
                    0 => f.access = FolderAccess::ReadOnly,
                    1 => f.access = FolderAccess::ReadWrite,
                    FOLDER_LOCATE => {
                        pick = Some(PickPurpose::RelocateFolder(game, f.canonical.clone()));
                    }
                    _ => {}
                }
            }
        }
        (Perm::UserDir(d, path), Picked::Switched(true)) => {
            match e.saved.folders.iter().position(|f| is_dir(f, d, &path)) {
                Some(j) => restore(e, j),
                None => e.toggle_user_dir(d, path, now),
            }
        }
        (Perm::Dropped(j), Picked::Switched(true)) => restore(e, j),
        _ => return,
    }
    e.reclassify();
    e.folder_error = None;
    state.picks.extend(pick);
    reassess(state, env);
}

/// A link inside a line of text: accent text, 500, underlined under the pointer.
fn text_link(ui: &mut Ui, text: &str) -> egui::Response {
    let theme = theme::theme(ui.ctx());
    let p = &theme.palette;
    let r = ui.add(
        egui::Label::new(Type::BodyStrong.rich(text).color(p.accent_text))
            .sense(egui::Sense::click()),
    );
    if r.hovered() || r.has_focus() {
        ui.painter().hline(
            r.rect.x_range(),
            r.rect.bottom() - stroke::BORDER,
            egui::Stroke::new(stroke::BORDER, p.accent_text),
        );
    }
    crate::a11y::labelled(ui.ctx(), r.id, egui::Role::Link, text);
    r
}

/// The level: the three as one control (nothing chosen while customised), "Custom, based on
/// Offline. Reset to Offline", and what the draft comes to.
fn level(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let Some(e) = editor(state) else { return };
    let base = e.draft.preset.base();
    let custom = e.draft.preset.is_custom();
    let labels: Vec<&str> = BasePreset::ALL
        .into_iter()
        .map(|b| strings::shield::preset_name(b.into()))
        .collect();
    let hints = BasePreset::ALL.map(strings::shield::base_description);
    let mut chosen = if custom {
        usize::MAX
    } else {
        BasePreset::ALL.iter().position(|b| *b == base).unwrap_or(0)
    };
    if segmented_with_hints(
        ui,
        s::PROTECTION_LEVEL(),
        &mut chosen,
        &segs(&labels),
        &hints,
    )
    .changed()
        && let Some(b) = BasePreset::ALL.get(chosen).copied()
    {
        if custom {
            // Choosing a level replaces the switches the user set: asked first.
            e.confirm_preset = Some(b);
        } else {
            e.draft = env.bridge.preset_policy(b, &e.draft);
        }
    }
    if custom {
        ui.add_space(gm::CUSTOM_TOP);
        // The line is as high as its text: the row's height is taken when it starts.
        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = Type::Body.line_height();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(Type::Body.rich(s::custom_line(base)).color(p.t3));
                let reset = s::reset_to(base);
                if text_link(ui, &reset).clicked() {
                    e.confirm_preset = None;
                    e.draft = env.bridge.preset_policy(base, &e.draft);
                    e.draft.folders.clear();
                    e.draft.save_data = SaveDataLocation::Internal;
                    e.reclassify();
                }
            });
        });
    }
    if let Some(b) = e.confirm_preset {
        ui.add_space(space::S2);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = space::S2;
            ui.label(Type::Body.rich(s::replace_custom(b)).color(p.t2));
            if ui.add(Button::new(s::REPLACE())).clicked() {
                e.draft = env.bridge.preset_policy(b, &e.draft);
                e.confirm_preset = None;
            }
            if ui.add(Button::ghost(strings::dialogs::CANCEL())).clicked() {
                e.confirm_preset = None;
            }
        });
    }
    ui.add_space(gm::REACH_LISTS_TOP);
    let (can, cant) = reach(&e.draft);
    rows::reach(ui, s::CAN_USE(), &can, s::CANT_USE(), &cant);
}

/// A column of rows under its heading. Returns the row acted on and what was done.
fn group(ui: &mut Ui, title: &str, list: &[Row]) -> Option<(Perm, Picked)> {
    rows::heading(ui, title, true);
    let mut out = None;
    for r in list {
        if let Some(picked) = row(ui, r)
            && let Some(perm) = &r.perm
        {
            out = Some((perm.clone(), picked));
        }
    }
    out
}

/// What Advanced holds: identity, the system-call filter, the layer's copy, direct sound. A
/// switch that protects is on while its permission is withheld.
fn advanced(ui: &mut Ui, e: &mut PolicyEditor) {
    let before = e.draft.permissions();
    let mut granted = before;
    let mut withhold = |ui: &mut Ui, bit: Permissions, label: &str, hint: &str| {
        let mut on = !granted.contains(bit);
        if SettingRow::new(label)
            .help(hint)
            .show(ui, |ui| switch(ui, &mut on, label))
            .changed()
        {
            granted.set(bit, !on);
        }
    };
    rows::heading(ui, s::IDENTITY(), false);
    withhold(
        ui,
        Permissions::COMPUTER_NAME,
        s::MASK_HOSTNAME(),
        s::MASK_HOSTNAME_BREAKS(),
    );
    withhold(
        ui,
        Permissions::MACHINE_ID,
        s::MASK_MACHINE_ID(),
        s::MASK_MACHINE_ID_BREAKS(),
    );
    withhold(
        ui,
        Permissions::HARDWARE_IDS,
        s::MASK_HARDWARE(),
        s::MASK_HARDWARE_BREAKS(),
    );
    withhold(
        ui,
        Permissions::UNFILTERED_SYSCALLS,
        s::SECCOMP(),
        s::SECCOMP_HINT(),
    );
    let h = &mut e.draft.hardening;
    let mut iso = match h.runtime_isolation {
        RuntimeIsolation::Auto => 0,
        RuntimeIsolation::Isolated => 1,
        RuntimeIsolation::Shared => 2,
    };
    let mut changed = SettingRow::new(s::LAYER_ISOLATION())
        .show(ui, |ui| {
            segmented(
                ui,
                s::LAYER_ISOLATION(),
                &mut iso,
                &segs(&[s::ISO_AUTO(), s::ISO_PRIVATE(), s::ISO_SHARED()]),
                false,
            )
        })
        .changed();
    if changed {
        h.runtime_isolation = [
            RuntimeIsolation::Auto,
            RuntimeIsolation::Isolated,
            RuntimeIsolation::Shared,
        ][iso.min(2)];
    }
    let mut direct = granted.contains(Permissions::DIRECT_SOUND);
    if SettingRow::new(s::DIRECT_SOUND())
        .help(s::DIRECT_SOUND_HINT())
        .show(ui, |ui| switch(ui, &mut direct, s::DIRECT_SOUND()))
        .changed()
    {
        granted.set(Permissions::DIRECT_SOUND, direct);
    }
    if granted != before {
        e.draft.set_permissions(granted);
        changed = true;
    }
    if changed {
        e.reclassify();
    }
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
    ui.allocate_exact_size(vec2(w, line.min_rect().height()), egui::Sense::hover());
    out
}

/// Show the Protection tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    let Some(e) = editor(state) else {
        if state.details.detail.show_skeleton(env.mono) {
            super::skeleton_lines(ui, 6);
        }
        return;
    };
    if !e.saved.is_supported_version() {
        issue_line(ui, s::POLICY_TOO_NEW(), true);
        return;
    }
    if g.activity != easyplay_core::model::Activity::Idle {
        let _ = note(
            ui,
            &Note {
                tone: NoteTone::Info,
                icon: icons::CLOCK,
                title: s::APPLIES_NEXT_TIME(),
                code: None,
                text: "",
                break_after_title: false,
                actions: &[],
            },
        );
        ui.add_space(gm::NOTE_TOP);
    }
    level(ui, state, env);
    reassess(state, env);
    let Some(e) = editor(state) else { return };
    let devices = devices(e, env);
    let files = files(e, env, g);
    let rejected = e.folder_error.as_deref().map(s::rejected);
    ui.add_space(gm::SWITCHES_TOP);

    let w = ui.available_width();
    let two = super::content_width(ui) >= layout::TWO_COL_BELOW;
    let [gap_y, gap_x] = layout::TWO_COL_GAP;
    let top = ui.cursor().min;
    let col_w = if two { (w - gap_x) / 2.0 } else { w };
    // Ids of their own, not counted from what precedes them: "Custom, based on …" coming
    // and going above must not change the switches' ids (and so take their focus away).
    let mut left = ui.new_child(
        UiBuilder::new()
            .scope_id(egui::Id::unique(("protection.devices", g.id)))
            .max_rect(Rect::from_min_size(top, vec2(col_w, f32::INFINITY)))
            .layout(Layout::top_down(Align::Min)),
    );
    left.set_width(col_w);
    let mut done = group(&mut left, s::INTERNET_AND_DEVICES(), &devices);
    let left_rect = left.min_rect();
    let files_top = if two {
        top + vec2(col_w + gap_x, 0.0)
    } else {
        pos2(top.x, left_rect.bottom() + gap_y)
    };
    let mut right = ui.new_child(
        UiBuilder::new()
            .scope_id(egui::Id::unique(("protection.files", g.id)))
            .max_rect(Rect::from_min_size(files_top, vec2(col_w, f32::INFINITY)))
            .layout(Layout::top_down(Align::Min)),
    );
    right.set_width(col_w);
    done = group(&mut right, s::FILES(), &files).or(done);
    // "+ Add folder…", its text on the column's axis.
    right.add_space(gm::ROW_BUTTONS[0]);
    let add = hanging(&mut right, |ui| {
        let b = ui.add(Button::ghost(s::ADD_FOLDER()).icon(icons::PLUS));
        crate::widgets::tooltip::tooltip(b, s::DROP_FOLDER(), None).clicked()
    });
    let files_rect = right.min_rect();
    state
        .dnd
        .target(files_rect, DropTarget::FolderList(Some(g.id)));
    ui.allocate_rect(
        Rect::from_min_max(
            top,
            pos2(top.x + w, left_rect.bottom().max(files_rect.bottom())),
        ),
        egui::Sense::hover(),
    );
    // A folder that was refused is explained where it was asked for.
    if let Some(rejected) = &rejected {
        ui.add_space(space::S2);
        issue_line(ui, rejected, true);
    }

    // Advanced.
    if let Some(e) = editor(state) {
        let n = s::advanced_changes(&e.saved, &e.draft);
        ui.add_space(layout::HEADING_GAP[0]);
        let open = e.advanced;
        let disclosure = hanging(ui, |ui| {
            ui.add(
                Button::ghost(&s::advanced(n))
                    .icon(if open {
                        icons::CARET_DOWN
                    } else {
                        icons::CARET_RIGHT
                    })
                    .size(Size::Md),
            )
            .clicked()
        });
        if disclosure {
            e.advanced = !e.advanced;
        }
        if e.advanced {
            advanced(ui, e);
        } else {
            // Closed, Advanced still says what it has opened up; each line opens it.
            let p = theme::theme(ui.ctx()).palette.clone();
            let mut open_it = false;
            for st in &e.assessment.statements {
                let shown_in_rows = matches!(
                    st.category,
                    AccessCategory::Network
                        | AccessCategory::OwnFolder
                        | AccessCategory::YourFolders
                        | AccessCategory::Display
                ) || (st.category == AccessCategory::Devices
                    && !st.key.ends_with("alsa"));
                if shown_in_rows || st.severity < StatementSeverity::Notice {
                    continue;
                }
                let c = match st.severity {
                    StatementSeverity::Warning => p.amber,
                    StatementSeverity::Danger => p.coral,
                    _ => p.t2,
                };
                let r = super::fine(ui, &st.text, c).interact(egui::Sense::click());
                crate::a11y::button(&r, &st.text);
                if r.hovered() || r.has_focus() {
                    ui.painter().hline(
                        r.rect.x_range(),
                        r.rect.bottom() - stroke::BORDER,
                        egui::Stroke::new(stroke::BORDER, c),
                    );
                }
                open_it |= r.clicked();
            }
            if open_it {
                e.advanced = true;
            }
        }
    }

    match done {
        Some((perm, picked)) => decide(state, env, g.id, perm, picked),
        None if add => state.picks.push(PickPurpose::FolderGrant(Some(g.id))),
        None => {}
    }
}
