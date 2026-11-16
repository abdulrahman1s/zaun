//! Protection tab (SCREEN_SPECS_2 §8, UI-017) and folder permissions (§9, UI-018) as a list
//! of permissions (ADR-0023): the preset, then one row for each thing the game may be
//! allowed, with its switch. A permission that comes in degrees (a folder that may be read
//! or changed) shows the degree beside its switch. Rows keep their places. Beneath:
//! Advanced, and the save bar (in the page). A rejected folder never enters the list: the
//! message names the rule instead.

use std::borrow::Cow;
use std::path::PathBuf;

use easyplay_core::ids::GameId;
use easyplay_core::model::{
    BasePreset, FolderAccess, FolderGrant, GameDirAccess, GameSummary, GrantOrigin, Permissions,
    Preset, RuntimeIsolation, SaveDataLocation, UserDir,
};
use easyplay_core::sandbox::{
    AccessCategory, AccessStatement, IssueCode, IssueSeverity, StatementSeverity,
};
use egui::{Align, Id, Layout, Ui, vec2};

use super::overview::tone;
use super::statement_icon;
use crate::app::{UiState, assess_draft};
use crate::bridge::{GameEdit, UiCommand};
use crate::demo::{grant_name, user_dir_name};
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::screens::Env;
use crate::strings;
use crate::strings::details as s;
use crate::theme::tokens::{icon as icon_size, layout, space};
use crate::theme::{self, Icon, Type, icons};
use crate::vm::details::PolicyEditor;
use crate::widgets::button::{Button, Size, Variant};
use crate::widgets::link_row::Tone;
use crate::widgets::permission_row::{
    Choice, Degree, Line, PermissionRow, Picked, Press, permission_rows,
};
use crate::widgets::segmented::{Segment, segmented, segmented_with_hints};
use crate::widgets::shield;
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

/// What a row of the tab is.
#[derive(Clone, Debug, PartialEq, Eq)]
enum What {
    /// A permission.
    Perm(Perm),
    /// *Add folder…*.
    AddFolder,
    /// Something that is decided under Advanced.
    Advanced,
    /// Something that cannot be decided.
    Fact,
}

/// A row before it is drawn.
struct Row {
    what: What,
    id: Id,
    icon: Icon,
    title: String,
    lines: Vec<(String, Tone)>,
    /// Whether the permission is on, when it can be off.
    switch: Option<bool>,
    /// Its degrees, the one in force, and what is shown for it.
    degree: Option<(Vec<Choice<'static>>, usize, String)>,
    tone: Tone,
    changed: bool,
    hint: Option<String>,
}

/// The degrees of a folder, and of removable drives.
const FOLDER_DEGREES: [&str; 2] = [s::CAN_READ, s::CAN_READ_CHANGE];
/// *Locate…*, which follows them on a folder that is gone.
const FOLDER_LOCATE: usize = 2;

impl Row {
    fn new(
        what: What,
        key: impl std::hash::Hash + std::fmt::Debug,
        icon: Icon,
        title: impl Into<String>,
    ) -> Self {
        Self {
            what,
            id: Id::unique("permission").with(key),
            icon,
            title: title.into(),
            lines: Vec::new(),
            switch: None,
            degree: None,
            tone: Tone::Normal,
            changed: false,
            hint: None,
        }
    }

    fn line(mut self, text: impl Into<String>, tone: Tone) -> Self {
        self.lines.push((text.into(), tone));
        self
    }

    fn switch(mut self, on: bool) -> Self {
        self.switch = Some(on);
        self
    }

    /// The degrees, and the one in force.
    fn degree(mut self, labels: &[&'static str], chosen: usize) -> Self {
        let value = labels.get(chosen).copied().unwrap_or_default();
        self.degree = Some((
            labels.iter().map(|l| Choice::new(l)).collect(),
            chosen,
            value.to_owned(),
        ));
        self
    }

    fn widget(&self) -> PermissionRow<'_> {
        let weight = self.lines.iter().map(|(_, t)| *t).max().unwrap_or_default();
        PermissionRow {
            id: self.id,
            icon: self.icon,
            tone: weight.max(self.tone),
            quiet: self.switch == Some(false),
            title: Cow::Borrowed(&self.title),
            lines: self
                .lines
                .iter()
                .map(|(text, tone)| Line::toned(text.as_str(), *tone))
                .collect(),
            mark: self.changed.then_some(s::CHANGED_MARK),
            switch: self.switch,
            degree: self.degree.as_ref().map(|(choices, chosen, value)| Degree {
                choices,
                chosen: *chosen,
                value: Cow::Borrowed(value),
                name: Cow::Owned(s::permission(&self.title, value)),
            }),
            press: match self.what {
                What::AddFolder => Press::Action,
                What::Advanced => Press::Link,
                What::Perm(_) | What::Fact => Press::Nothing,
            },
            name: Cow::Borrowed(&self.title),
            hint: self.hint.as_deref().map(Cow::Borrowed),
            delete: matches!(self.what, What::Perm(Perm::Folder(_))),
        }
    }
}

/// The rows of the tab, in reading order.
#[derive(Default)]
struct Rows {
    /// The internet, the devices, and where the game draws.
    devices: Vec<Row>,
    /// The game's folder and the user's.
    files: Vec<Row>,
    /// What Advanced has opened up, in the statements' words.
    advanced: Vec<Row>,
}

/// Whether a statement is covered by a row of its own; the others are decided under
/// Advanced and are listed as they are.
fn has_row(st: &AccessStatement) -> bool {
    match st.category {
        AccessCategory::Network
        | AccessCategory::OwnFolder
        | AccessCategory::YourFolders
        | AccessCategory::Display => true,
        AccessCategory::Devices => !st.key.ends_with("alsa"),
        _ => false,
    }
}

/// The heaviest of the statements `of` picks.
fn weight_of(statements: &[AccessStatement], of: impl Fn(&AccessStatement) -> bool) -> Tone {
    statements
        .iter()
        .filter(|st| of(st))
        .map(|st| tone(st.severity))
        .max()
        .unwrap_or_default()
}

/// The row of a permission that is granted or not. `degrees` names the two degrees of one
/// that has them, the narrower first; `said` is the part of a statement's key that speaks
/// of it.
fn flag_row(
    e: &PolicyEditor,
    bit: Permissions,
    icon: Icon,
    title: &str,
    degrees: Option<[&'static str; 2]>,
    said: &[&str],
) -> Row {
    let (was, now) = (e.saved.permissions(), e.draft.permissions());
    let on = now.contains(bit);
    // A degree shown as a row of its own (the microphone) is that row's change.
    let wider = if degrees.is_some() {
        bit.degrees()
    } else {
        Permissions::empty()
    };
    let mut row = Row::new(What::Perm(Perm::Flag(bit)), bit.bits(), icon, title).switch(on);
    if let (true, Some(degrees)) = (on, degrees) {
        row = row.degree(&degrees, usize::from(now.contains(wider)));
    }
    row.tone = weight_of(&e.assessment.statements, |st| {
        said.iter().any(|part| st.key.contains(part))
    });
    row.changed = (was ^ now).intersects(bit | wider);
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
        What::Perm(Perm::Folder(i)),
        &f.canonical,
        icons::FOLDER,
        grant_name(f),
    )
    .switch(true)
    .degree(&FOLDER_DEGREES, usize::from(writable));
    if own {
        row = row.line(
            s::folder_location(&f.path, &env.bridge.host().home),
            Tone::Normal,
        );
    }
    if about.iter().any(|x| x.code == IssueCode::PathMissing) {
        if let Some((choices, ..)) = &mut row.degree {
            choices.push(Choice::new(s::LOCATE_FOLDER));
        }
        row = row.line(s::FOLDER_NOT_FOUND, Tone::Danger);
    }
    for x in about.iter().filter(|x| x.code != IssueCode::PathMissing) {
        let weight = if x.severity >= IssueSeverity::Error {
            Tone::Danger
        } else {
            Tone::Warning
        };
        row = row.line(issue_text(&x.message), weight);
    }
    if writable && e.draft.permissions().contains(Permissions::INTERNET) {
        row = row.line(s::WRITABLE_WITH_INTERNET, Tone::Warning);
    }
    let path = f.canonical.display().to_string();
    row.tone = weight_of(&e.assessment.statements, |st| {
        st.args.get("path") == Some(&path)
    });
    row.changed = !e
        .saved
        .folders
        .iter()
        .any(|x| x.canonical == f.canonical && x.access == f.access);
    row.hint = Some(f.path.display().to_string());
    row
}

/// The game's folder and the user's, in an order that no switch changes: the game's own,
/// the well-known folders, the folders the user added, removable drives.
fn files(e: &PolicyEditor, env: &Env<'_>, g: &GameSummary) -> Vec<Row> {
    let host = env.bridge.host();
    let (saved, draft) = (&e.saved, &e.draft);
    let mut out = Vec::new();

    let (name, parent) = s::location(g.root.as_deref(), &g.primary_exe_path);
    let place = s::place(&name, parent.as_deref());
    if let Some(root) = &g.root {
        let chosen = match draft.game_dir {
            GameDirAccess::ReadWrite => 0,
            GameDirAccess::ReadOnly | GameDirAccess::FileOnly => 1,
            GameDirAccess::Protected => 2,
        };
        let mut row = Row::new(
            What::Perm(Perm::GameFolder),
            "game-folder",
            icons::FOLDER,
            s::GAME_FOLDER,
        )
        .degree(
            &[s::CAN_READ_CHANGE, s::CAN_READ, s::CHANGES_KEPT_APART],
            chosen,
        )
        .line(place, Tone::Normal);
        row.changed = saved.game_dir != draft.game_dir;
        row.hint = Some(root.display().to_string());
        out.push(row);
    } else {
        let row = Row::new(What::Fact, "game-folder", icons::FILE_EXE, s::PROGRAM_FILE)
            .line(place, Tone::Normal)
            .line(s::FILE_ONLY_EXPLAINED, Tone::Normal);
        out.push(row);
    }

    let external = matches!(draft.save_data, SaveDataLocation::External { .. });
    let mut row = Row::new(
        What::Perm(Perm::SaveData),
        "save-data",
        icons::FLOPPY,
        s::SAVE_DATA,
    )
    .degree(
        &[s::PRIVATE_STORAGE, s::CHOOSE_FOLDER],
        usize::from(external),
    );
    if let SaveDataLocation::External { dir } = &draft.save_data {
        // The folder is named, in place of the choice that led to it.
        if let Some((.., value)) = &mut row.degree {
            *value = dir.file_name().map_or_else(
                || dir.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
        }
        row = row.line(s::folder_location(dir, &host.home), Tone::Normal);
        row.hint = Some(dir.display().to_string());
    }
    row.changed = saved.save_data != draft.save_data;
    out.push(row);

    let dirs: Vec<(UserDir, PathBuf)> = USER_DIRS
        .into_iter()
        .filter_map(|d| user_dir_path(env, d).map(|p| (d, p)))
        .collect();
    let known = |f: &FolderGrant| dirs.iter().any(|(d, path)| is_dir(f, *d, path));
    for (d, path) in &dirs {
        if let Some(i) = draft.folders.iter().position(|f| is_dir(f, *d, path)) {
            out.push(folder_row(e, env, i, &draft.folders[i], false));
            continue;
        }
        let mut row = Row::new(
            What::Perm(Perm::UserDir(*d, path.clone())),
            path,
            icons::FOLDER,
            user_dir_name(*d),
        )
        .switch(false);
        row.changed = saved.folders.iter().any(|f| is_dir(f, *d, path));
        row.hint = Some(path.display().to_string());
        out.push(row);
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
        let mut row = Row::new(
            What::Perm(Perm::Dropped(j)),
            &f.canonical,
            icons::FOLDER,
            grant_name(f),
        )
        .switch(false)
        .line(s::folder_location(&f.path, &host.home), Tone::Normal);
        row.changed = true;
        row.hint = Some(f.path.display().to_string());
        out.push(row);
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
        icons::USB,
        s::REMOVABLE,
        Some(FOLDER_DEGREES),
        &["removable"],
    ));

    let add = Row::new(
        What::AddFolder,
        "add-folder",
        icons::FOLDER_PLUS,
        s::ADD_FOLDER,
    );
    out.push(add.line(s::DROP_FOLDER, Tone::Normal));
    out
}

/// The rows of the draft.
fn rows(e: &PolicyEditor, env: &Env<'_>, g: &GameSummary) -> Rows {
    let host = env.bridge.host();
    let statements = &e.assessment.statements;
    let online = e.draft.permissions().contains(Permissions::INTERNET);
    let globe = if online {
        icons::GLOBE
    } else {
        icons::WIFI_OFF
    };
    let mut controllers = flag_row(
        e,
        Permissions::CONTROLLERS,
        icons::GAMEPAD,
        s::GAME_CONTROLLERS,
        Some([s::CONNECTED_ONLY, s::ALL_INPUT]),
        &["controllers", "all_input"],
    );
    if let (true, Some((choices, ..))) = (host.input_readable, &mut controllers.degree) {
        choices[1].unavailable = Some(s::ALL_INPUT_UNAVAILABLE);
    }
    let mut microphone = flag_row(
        e,
        Permissions::MICROPHONE,
        icons::MIC,
        s::MICROPHONE,
        None,
        &["microphone"],
    );
    let sound = e.draft.permissions().contains(Permissions::SOUND);
    if host.sound_includes_microphone && sound && microphone.switch != Some(true) {
        microphone = microphone.line(s::MICROPHONE_INSEPARABLE, Tone::Warning);
    }
    let mut devices = vec![
        flag_row(e, Permissions::INTERNET, globe, s::INTERNET_ON, None, &[]),
        flag_row(
            e,
            Permissions::SOUND,
            icons::SPEAKER,
            s::SOUND,
            None,
            &["sound"],
        ),
        microphone,
        controllers,
        flag_row(
            e,
            Permissions::CAMERA,
            icons::CAMERA,
            s::CAMERA,
            None,
            &["camera"],
        ),
    ];

    // Where the game draws cannot be decided here; what it means is said.
    let mut screen = Row::new(What::Fact, "screen", icons::MONITOR, s::SCREEN);
    let mut x11 = false;
    for st in statements {
        if st.category == AccessCategory::Display {
            x11 |= st.key.contains("x11");
            screen = screen.line(&st.text, tone(st.severity));
        }
    }
    if host.x11 && !x11 {
        screen = screen.line(s::X11_NOTICE, Tone::Warning);
    }
    if !screen.lines.is_empty() {
        devices.push(screen);
    }

    let mut advanced = Vec::new();
    for st in statements {
        if has_row(st) || st.severity < StatementSeverity::Notice {
            continue;
        }
        let mut row = Row::new(
            What::Advanced,
            (&st.key, &st.text),
            statement_icon(st),
            &st.text,
        );
        if let Some(detail) = st.detail.as_deref().filter(|d| !d.is_empty()) {
            row = row.line(detail, Tone::Normal);
        }
        row.tone = tone(st.severity);
        advanced.push(row);
    }
    Rows {
        devices,
        files: files(e, env, g),
        advanced,
    }
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
        (Perm::SaveData, Picked::Degree(_)) => pick = Some(PickPurpose::SaveFolder(Some(game))),
        (Perm::Folder(i), Picked::Switched(false) | Picked::Delete) => {
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

/// The preset: the three as one control, and what the chosen one means beside it.
fn preset(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let Some(e) = editor(state) else { return };
    let h = ui.label(
        Type::Heading
            .rich(s::PROTECTION_LEVEL)
            .color(p.text_primary),
    );
    crate::a11y::labelled(ui.ctx(), h.id, egui::Role::Heading, s::PROTECTION_LEVEL);
    ui.add_space(space::S1);
    let base = e.draft.preset.base();
    let options: Vec<Segment<'_>> = BasePreset::ALL
        .into_iter()
        .map(|b| Segment {
            icon: Some(shield::glyph(b.into(), false)),
            label: strings::shield::preset_name(b.into()),
        })
        .collect();
    let hints = BasePreset::ALL.map(strings::shield::base_description);
    let mut chosen = BasePreset::ALL.iter().position(|b| *b == base).unwrap_or(0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = space::S3;
        if segmented_with_hints(ui, s::PROTECTION_LEVEL, &mut chosen, &options, &hints).changed() {
            let b = BasePreset::ALL[chosen];
            if e.draft.preset.is_custom() {
                e.confirm_preset = Some(b);
            } else {
                e.draft = env.bridge.preset_policy(b, &e.draft);
            }
        }
        let Preset::Custom { base } = e.draft.preset else {
            // Set whole on the next line when the control leaves no room beside it.
            let means = strings::shield::base_description(base);
            ui.add(egui::Label::new(Type::Body.rich(means).color(p.text_secondary)).extend());
            return;
        };
        let custom = Type::Body
            .rich(s::custom_based_on(base))
            .color(p.text_secondary);
        ui.add(egui::Label::new(custom).extend());
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
    });
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
}

/// One headed list of rows. Returns the row acted on and what was done.
fn list(ui: &mut Ui, title: &str, rows: &[Row]) -> Option<(What, Picked)> {
    section(ui, title, None);
    let shown: Vec<PermissionRow<'_>> = rows.iter().map(Row::widget).collect();
    let list = ui.scope(|ui| permission_rows(ui, &shown));
    crate::a11y::labelled(ui.ctx(), list.response.id, egui::Role::List, title);
    list.inner.map(|(i, picked)| (rows[i].what.clone(), picked))
}

/// What Advanced holds: identity, the system-call filter, the layer's copy, direct sound.
/// A switch that protects is on while its permission is withheld.
fn advanced(ui: &mut Ui, e: &mut PolicyEditor) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let before = e.draft.permissions();
    let mut granted = before;
    let mut withhold = |ui: &mut Ui, bit: Permissions, label: &str, hint: &str| {
        let mut on = !granted.contains(bit);
        if toggle(ui, &mut on, label, Some(hint)).changed() {
            granted.set(bit, !on);
        }
    };
    ui.label(Type::BodyStrong.rich(s::IDENTITY).color(p.text_primary));
    withhold(
        ui,
        Permissions::COMPUTER_NAME,
        s::MASK_HOSTNAME,
        s::MASK_HOSTNAME_BREAKS,
    );
    withhold(
        ui,
        Permissions::MACHINE_ID,
        s::MASK_MACHINE_ID,
        s::MASK_MACHINE_ID_BREAKS,
    );
    withhold(
        ui,
        Permissions::HARDWARE_IDS,
        s::MASK_HARDWARE,
        s::MASK_HARDWARE_BREAKS,
    );
    withhold(
        ui,
        Permissions::UNFILTERED_SYSCALLS,
        s::SECCOMP,
        s::SECCOMP_HINT,
    );
    ui.label(
        Type::BodyStrong
            .rich(s::LAYER_ISOLATION)
            .color(p.text_primary),
    );
    let h = &mut e.draft.hardening;
    let mut iso = match h.runtime_isolation {
        RuntimeIsolation::Auto => 0,
        RuntimeIsolation::Isolated => 1,
        RuntimeIsolation::Shared => 2,
    };
    let mut changed = segmented(
        ui,
        s::LAYER_ISOLATION,
        &mut iso,
        &segs(&[s::ISO_AUTO, s::ISO_PRIVATE, s::ISO_SHARED]),
        false,
    )
    .changed();
    if changed {
        h.runtime_isolation = [
            RuntimeIsolation::Auto,
            RuntimeIsolation::Isolated,
            RuntimeIsolation::Shared,
        ][iso];
    }
    let mut direct = granted.contains(Permissions::DIRECT_SOUND);
    if toggle(ui, &mut direct, s::DIRECT_SOUND, Some(s::DIRECT_SOUND_HINT)).changed() {
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

/// Show the Protection tab.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
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
    // A narrower measure than the tabs of fields: a title and its switch in one glance.
    let w = ui.available_width().min(layout::PERMISSIONS_MAX_W);
    ui.allocate_ui_with_layout(vec2(w, 0.0), Layout::top_down(Align::Min), |ui| {
        ui.set_width(w);
        body(ui, state, env, g);
    });
}

/// The preset, the permissions and Advanced.
fn body(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, g: &GameSummary) {
    preset(ui, state, env);
    reassess(state, env);
    let Some(e) = editor(state) else { return };
    let rows = rows(e, env, g);
    let rejected = e.folder_error.as_deref().map(s::rejected);

    let mut done = list(ui, s::INTERNET_AND_DEVICES, &rows.devices);
    let top = ui.cursor().min;
    done = list(ui, s::FILES, &rows.files).or(done);
    let files = egui::Rect::from_min_max(top, ui.min_rect().right_bottom());
    state.dnd.target(files, DropTarget::FolderList(Some(g.id)));
    // A folder that was refused is explained where it was asked for.
    if let Some(rejected) = &rejected {
        issue_line(ui, rejected, true);
    }

    let Some(e) = editor(state) else { return };
    ui.add_space(space::S4);
    let n = s::advanced_changes(&e.saved, &e.draft);
    let disclosure = ui.add(Button::ghost(&s::advanced(n)).icon(if e.advanced {
        icons::CARET_DOWN
    } else {
        icons::CARET_RIGHT
    }));
    if disclosure.clicked() {
        e.advanced = !e.advanced;
    }
    let inset = egui::Frame::NONE.inner_margin(egui::Margin {
        left: space::S6 as i8,
        ..Default::default()
    });
    if e.advanced {
        inset.show(ui, |ui| advanced(ui, e));
    } else if !rows.advanced.is_empty() {
        // Closed, Advanced still says what it has opened up.
        let shown: Vec<PermissionRow<'_>> = rows.advanced.iter().map(Row::widget).collect();
        if inset
            .show(ui, |ui| permission_rows(ui, &shown))
            .inner
            .is_some()
        {
            e.advanced = true;
        }
    }

    match done {
        Some((What::Perm(perm), picked)) => decide(state, env, g.id, perm, picked),
        Some((What::AddFolder, _)) => state.picks.push(PickPurpose::FolderGrant(Some(g.id))),
        _ => {}
    }
}
