//! Protection tab (zaun-redesign.html `tabProtection`, `PROWS`, `prowHTML`, `sharedHTML`,
//! `plistHTML`, `protSync`; ADR-0030). A grouped list a first-time user reads top to bottom
//! without hovering, in the 620 column: one sentence, then *Internet and devices*, *Its own
//! files* and *Your files* under the Overview's headings, each with a short count at its
//! right. Every row has an icon tile (lit in the accent while the game may use the thing),
//! its name, one plain line (a folder's full path in mono), and its control at the right.
//! There are no levels: the stored `preset` is kept up to date for the library and never
//! shown here. Below the list: "+ Share another folder…", while the game runs a line that
//! changes apply next time, then the Advanced disclosure with the rest as setting rows.
//!
//! Where the stored policy can say more than a row shows (ADR-0030 records each choice):
//! the game folder's "changes kept apart" is a third item of its select; an external save
//! folder is an item of the Save data select, its path the row's line; how far a folder or
//! removable drives may go is in the *Shared* menu of a folder the user added and in
//! Advanced for the home folders and removable drives (a row that may change files says so
//! in a third line); the microphone that sound can't leave out, a folder's issues and a
//! folder that can't be found are a third line on their row; "All input devices" where it
//! can't be had is an unavailable item of the controllers' menu, with the reason; the
//! screen is a row in Advanced; a refused folder is explained under "+ Share another
//! folder…". Changes are a draft saved with the page's save bar, as on the other tabs.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Align2, Color32, CornerRadius, FontFamily, FontId, Galley, Id, Layout, Popup,
    PopupCloseBehavior, Rect, Response, Sense, Shape, Ui, UiBuilder, pos2, vec2,
};
use zaun_core::ids::GameId;
use zaun_core::model::{
    FolderAccess, FolderGrant, GameDirAccess, GameSummary, GrantOrigin, Permissions,
    RuntimeIsolation, SaveDataLocation, UserDir,
};
use zaun_core::sandbox::{AccessCategory, IssueCode, IssueSeverity, StatementSeverity};

use crate::app::{UiState, assess_draft};
use crate::bridge::{GameEdit, UiCommand};
use crate::demo::grant_name;
use crate::i18n::bidi;
use crate::input::dnd::DropTarget;
use crate::input::picker::PickPurpose;
use crate::screens::Env;
use crate::strings;
use crate::strings::details as s;
use crate::theme::palette::mix;
use crate::theme::tokens::{control, game as gm, icon as icon_size, layout, radius, space, stroke};
use crate::theme::{self, Dur, Icon, Type, icons};
use crate::vm::details::PolicyEditor;

pub use crate::vm::details::USER_DIRS;
use crate::widgets::rows::{self, SettingRow};
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::toggle::switch;
use crate::widgets::{context_menu, contrast_outline, focus_ring, glyph_shape, label_galley};

/// Where a well-known folder lives on this host.
pub fn user_dir_path(env: &Env<'_>, d: UserDir) -> Option<PathBuf> {
    env.bridge
        .host()
        .user_dirs
        .iter()
        .find(|(x, _)| *x == d)
        .map(|(_, p)| p.clone())
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

fn reject_denied(state: &mut UiState, env: &Env<'_>, path: &Path) {
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
pub fn relocate_folder(state: &mut UiState, game: GameId, old: &Path, new: PathBuf) {
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
        let advanced = e.advanced;
        *e = PolicyEditor::new(saved, a);
        e.advanced = advanced;
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
            d.game.launch = zaun_core::model::LaunchConfig {
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

// ---- The rows ------------------------------------------------------------------------------

/// What a row stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Perm {
    /// A permission that is granted or not: its bit.
    Flag(Permissions),
    /// A home folder, and where it is on this host.
    UserDir(UserDir, PathBuf),
    /// The game's own folder.
    GameFolder,
    /// Where save data is kept.
    SaveData,
    /// A folder the user shared: its place.
    Shared(PathBuf),
    /// Nothing to decide (a game that is a single program).
    Fixed,
}

/// The second line of a row: a plain sentence, or a folder's full path.
enum Line {
    Text(String),
    Path(String),
}

/// How heavily a third line weighs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Weight {
    Quiet,
    Warning,
    Danger,
}

/// An item of a select's menu that is an action, not a choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    /// Find a shared folder that is gone.
    Locate,
    /// Take a shared folder away.
    StopSharing,
}

/// The select a row carries: how the choice reads in the row, the full wording in its menu.
struct Choice {
    /// On the button ("Connected at start").
    short: String,
    /// The accessible name ("Game controllers: Connected when the game starts").
    name: String,
    /// The choices, in full.
    options: Vec<String>,
    /// The one in force.
    chosen: Option<usize>,
    /// A choice that can't be made here, and why.
    unavailable: Option<(String, String)>,
    /// Actions after the choices.
    actions: Vec<(String, Act)>,
}

/// What was done to a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Did {
    Switched(bool),
    Chose(usize),
    Act(Act),
}

/// A row before it is drawn.
struct Row {
    key: String,
    perm: Perm,
    icon: Icon,
    name: String,
    /// The switch's name ("Pictures folder").
    spoken: String,
    line: Line,
    extra: Vec<(String, Weight)>,
    /// Allowed: the tile is lit.
    on: bool,
    /// Has a switch (else it is always allowed and the row does not highlight).
    switch: bool,
    /// Shown while on.
    choice: Option<Choice>,
    /// Differs from what is saved (said to assistive technology as "changed").
    changed: bool,
}

impl Row {
    fn new(key: impl Into<String>, perm: Perm, icon: Icon, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            key: key.into(),
            perm,
            icon,
            spoken: name.clone(),
            name,
            line: Line::Text(String::new()),
            extra: Vec::new(),
            on: true,
            switch: false,
            choice: None,
            changed: false,
        }
    }

    fn text(mut self, line: &str) -> Self {
        self.line = Line::Text(line.to_owned());
        self
    }

    fn path(mut self, path: &Path) -> Self {
        self.line = Line::Path(path.display().to_string());
        self
    }

    fn switch(mut self, on: bool) -> Self {
        self.on = on;
        self.switch = true;
        self
    }

    fn extra(mut self, text: impl Into<String>, weight: Weight) -> Self {
        self.extra.push((text.into(), weight));
        self
    }
}

/// A choice of several, each said in full in the menu and short in the row.
fn choice(title: &str, full: &[&str], short: &[&str], chosen: usize) -> Choice {
    let k = chosen.min(full.len().saturating_sub(1));
    Choice {
        short: short.get(k).copied().unwrap_or_default().to_owned(),
        name: s::permission(title, full.get(k).copied().unwrap_or_default()),
        options: full.iter().map(|x| (*x).to_owned()).collect(),
        chosen: Some(k),
        unavailable: None,
        actions: Vec::new(),
    }
}

/// Whether a grant is for this home folder.
fn is_dir(f: &FolderGrant, d: UserDir, path: &Path) -> bool {
    f.origin == GrantOrigin::WellKnown(d) || f.canonical == path
}

/// The home folders of this host, with where they are.
fn home_dirs(env: &Env<'_>) -> Vec<(UserDir, PathBuf)> {
    USER_DIRS
        .into_iter()
        .filter_map(|d| user_dir_path(env, d).map(|p| (d, p)))
        .collect()
}

/// The issues about a folder.
fn folder_issues<'a>(
    e: &'a PolicyEditor,
    f: &'a FolderGrant,
) -> impl Iterator<Item = &'a zaun_core::sandbox::PolicyIssue> {
    e.assessment.issues.iter().filter(|x| {
        x.path.as_deref() == Some(f.canonical.as_path()) || x.path.as_deref() == Some(&f.path)
    })
}

/// The third lines of a shared folder's row: its issues, then whether it may change files.
fn folder_lines(e: &PolicyEditor, f: &FolderGrant, mut row: Row) -> Row {
    for x in folder_issues(e, f) {
        if x.code == IssueCode::PathMissing {
            row = row.extra(s::FOLDER_MISSING(), Weight::Warning);
            continue;
        }
        let weight = if x.severity >= IssueSeverity::Error {
            Weight::Danger
        } else {
            Weight::Warning
        };
        row = row.extra(issue_text(&x.message), weight);
    }
    if f.access == FolderAccess::ReadWrite {
        row = if e.draft.permissions().contains(Permissions::INTERNET) {
            row.extra(s::WRITABLE_WITH_INTERNET(), Weight::Warning)
        } else {
            row.extra(s::CAN_CHANGE_HERE(), Weight::Quiet)
        };
    }
    row
}

/// The row of a permission that is granted or not.
fn flag_row(e: &PolicyEditor, bit: Permissions, icon: Icon, name: &str, line: &str) -> Row {
    let now = e.draft.permissions();
    let all = bit | bit.degrees();
    let mut row = Row::new(format!("flag-{}", bit.bits()), Perm::Flag(bit), icon, name)
        .text(line)
        .switch(now.contains(bit));
    row.changed = e.saved.permissions() & all != now & all;
    row
}

/// *Internet and devices*.
fn devices(e: &PolicyEditor, env: &Env<'_>) -> Vec<Row> {
    let host = env.bridge.host();
    let granted = e.draft.permissions();
    let mut microphone = flag_row(
        e,
        Permissions::MICROPHONE,
        icons::MIC,
        s::MICROPHONE(),
        s::MICROPHONE_LINE(),
    );
    if host.sound_includes_microphone
        && granted.contains(Permissions::SOUND)
        && !granted.contains(Permissions::MICROPHONE)
    {
        microphone = microphone.extra(s::MICROPHONE_INSEPARABLE(), Weight::Warning);
    }
    let mut controllers = flag_row(
        e,
        Permissions::CONTROLLERS,
        icons::GAMEPAD,
        s::GAME_CONTROLLERS(),
        s::CONTROLLERS_LINE(),
    );
    let all = granted.contains(Permissions::ALL_INPUT);
    let mut c = choice(
        s::GAME_CONTROLLERS(),
        &[s::CONNECTED_ONLY(), s::ALL_INPUT()],
        &[s::CONNECTED_AT_START(), s::ALL_DEVICES()],
        usize::from(all),
    );
    // All input devices is offered only where the account cannot read keyboards directly.
    if host.input_readable && !all {
        c.options.truncate(1);
        c.unavailable = Some((
            s::ALL_INPUT().to_owned(),
            s::ALL_INPUT_UNAVAILABLE().to_owned(),
        ));
    }
    controllers.choice = Some(c);
    vec![
        flag_row(
            e,
            Permissions::INTERNET,
            icons::WIFI,
            s::INTERNET_ON(),
            s::INTERNET_LINE(),
        ),
        flag_row(
            e,
            Permissions::SOUND,
            icons::SPEAKER,
            s::SOUND(),
            s::SOUND_LINE(),
        ),
        microphone,
        controllers,
        flag_row(
            e,
            Permissions::CAMERA,
            icons::CAMERA,
            s::CAMERA(),
            s::CAMERA_LINE(),
        ),
    ]
}

/// *Its own files*: the game folder and the save data, always allowed.
fn own(e: &PolicyEditor, g: &GameSummary) -> Vec<Row> {
    let (saved, draft) = (&e.saved, &e.draft);
    let mut out = Vec::new();
    match g.root.as_deref() {
        Some(root) => {
            let chosen = match draft.game_dir {
                GameDirAccess::ReadWrite => 0,
                GameDirAccess::ReadOnly | GameDirAccess::FileOnly => 1,
                GameDirAccess::Protected => 2,
            };
            let mut row = Row::new(
                "game-folder",
                Perm::GameFolder,
                icons::FOLDER,
                s::GAME_FOLDER(),
            )
            .path(root);
            row.choice = Some(choice(
                s::GAME_FOLDER(),
                &[
                    s::CAN_READ_CHANGE(),
                    s::CAN_ONLY_READ(),
                    s::CHANGES_KEPT_APART(),
                ],
                &[s::READ_AND_CHANGE(), s::READ_ONLY_SHORT(), s::KEPT_APART()],
                chosen,
            ));
            row.changed = saved.game_dir != draft.game_dir;
            out.push(row);
        }
        None => out.push(
            Row::new(
                "game-folder",
                Perm::Fixed,
                icons::FILE_EXE,
                s::PROGRAM_FILE(),
            )
            .path(&g.primary_exe_path)
            .extra(s::FILE_ONLY_EXPLAINED(), Weight::Quiet),
        ),
    }
    let mut row = Row::new("save-data", Perm::SaveData, icons::FLOPPY, s::SAVE_DATA());
    row.changed = saved.save_data != draft.save_data;
    match &draft.save_data {
        SaveDataLocation::External { dir } => {
            // The folder is named, in place of the choice that led to it.
            let name = dir.file_name().map_or_else(
                || dir.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            row = row.path(dir);
            row.choice = Some(Choice {
                short: name.clone(),
                name: s::permission(s::SAVE_DATA(), &name),
                options: vec![
                    s::PRIVATE_STORAGE().to_owned(),
                    name,
                    s::CHOOSE_FOLDER().to_owned(),
                ],
                chosen: Some(1),
                unavailable: None,
                actions: Vec::new(),
            });
        }
        _ => {
            row = row.text(s::SAVES_LINE());
            row.choice = Some(choice(
                s::SAVE_DATA(),
                &[s::PRIVATE_STORAGE(), s::CHOOSE_FOLDER()],
                &[s::PRIVATE_SHORT(), s::CHOOSE_FOLDER()],
                0,
            ));
        }
    }
    out.push(row);
    out
}

/// *Your files*: the home folders, removable drives, then the folders the user shared.
fn yours(e: &PolicyEditor, env: &Env<'_>) -> Vec<Row> {
    let (saved, draft) = (&e.saved, &e.draft);
    let dirs = home_dirs(env);
    let mut out = Vec::new();
    for (d, path) in &dirs {
        let name = s::user_dir(*d);
        // The row keeps its key (and its switch the focus) whether the folder is shared.
        let mut row = Row::new(
            format!("user-dir-{d:?}"),
            Perm::UserDir(*d, path.clone()),
            icons::FOLDER,
            name,
        )
        .path(path);
        row.spoken = s::folder_named(name);
        match draft.folders.iter().find(|f| is_dir(f, *d, path)) {
            Some(f) => {
                row = folder_lines(e, f, row.switch(true));
                row.changed = !saved
                    .folders
                    .iter()
                    .any(|x| x.canonical == f.canonical && x.access == f.access);
            }
            None => {
                row = row.switch(false);
                row.changed = saved.folders.iter().any(|f| is_dir(f, *d, path));
            }
        }
        out.push(row);
    }
    let removable = draft.permissions().contains(Permissions::REMOVABLE);
    let mut row = flag_row(
        e,
        Permissions::REMOVABLE,
        icons::HARD_DRIVE,
        s::REMOVABLE(),
        s::REMOVABLE_LINE(),
    );
    if removable && draft.permissions().contains(Permissions::REMOVABLE_WRITE) {
        row = row.extra(s::CAN_CHANGE_HERE(), Weight::Quiet);
    }
    out.push(row);
    let known = |f: &FolderGrant| dirs.iter().any(|(d, path)| is_dir(f, *d, path));
    for f in draft.folders.iter().filter(|f| !known(f)) {
        let name = grant_name(f);
        let mut row = Row::new(
            f.canonical.display().to_string(),
            Perm::Shared(f.canonical.clone()),
            icons::FOLDER,
            &name,
        )
        .path(&f.path);
        row.changed = !saved
            .folders
            .iter()
            .any(|x| x.canonical == f.canonical && x.access == f.access);
        let missing = folder_issues(e, f).any(|x| x.code == IssueCode::PathMissing);
        let mut actions = Vec::new();
        if missing {
            actions.push((s::LOCATE_FOLDER().to_owned(), Act::Locate));
        }
        actions.push((s::STOP_SHARING().to_owned(), Act::StopSharing));
        row.choice = Some(Choice {
            short: s::SHARED().to_owned(),
            name: s::shared_options(&name),
            options: vec![s::CAN_READ().to_owned(), s::CAN_READ_CHANGE().to_owned()],
            chosen: Some(usize::from(f.access == FolderAccess::ReadWrite)),
            unavailable: None,
            actions,
        });
        out.push(folder_lines(e, f, row));
    }
    out
}

// ---- Drawing -------------------------------------------------------------------------------

/// A folder's full path: mono 11 / 16, broken anywhere, always left to right (a path is
/// never reordered by the interface's direction).
fn path_galley(ui: &Ui, path: &str, color: Color32, width: f32) -> Arc<Galley> {
    let [size, line] = gm::PROT_PATH;
    let rtl = bidi::has_rtl(path);
    let mut job = LayoutJob::single_section(
        path_text(path),
        egui::TextFormat {
            font_id: FontId::new(size, FontFamily::Monospace),
            line_height: Some(line),
            color,
            ..Default::default()
        },
    );
    // A path with right-to-left names is put in order as one line, so it stays one line.
    job.wrap = TextWrapping {
        max_width: width.max(1.0),
        max_rows: if rtl { 1 } else { usize::MAX },
        break_anywhere: true,
        ..TextWrapping::default()
    };
    ui.fonts_mut(|f| f.layout_job(job))
}

/// A path put in the order the engine must be given: left to right whatever the interface's
/// direction, so "/home/sam/الصور" never reads with its separators reordered.
fn path_text(path: &str) -> String {
    bidi::visual_line(path, Some(crate::i18n::Dir::Ltr)).into_owned()
}

/// The width of a row's select showing `short`.
fn choice_width(ui: &Ui, short: &str) -> f32 {
    let g = label_galley(
        ui.painter(),
        short,
        Type::Body.font_id(),
        Type::Body.line_height(),
    );
    let [pad_l, pad_r] = control::SELECT_PAD;
    pad_l + g.size().x + layout::WORD_GAP + icon_size::SMALL + pad_r
}

/// A row's select (`.sel`): 28 high on `o1` (`o2` under the pointer and while open), the
/// short wording and a caret; its menu has the choices in full, then any actions.
fn pick(ui: &mut Ui, c: &Choice) -> (Response, Option<Did>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let painter = ui.painter().clone();
    let g = label_galley(
        &painter,
        &c.short,
        Type::Body.font_id(),
        Type::Body.line_height(),
    );
    let w = choice_width(ui, &c.short);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, control::SM), Sense::click());
    let open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(&resp));
    let enabled = resp.enabled();
    let name = c.name.clone();
    resp.widget_info(move || egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, &name));
    if ui.is_rect_visible(rect) {
        let hover = crate::widgets::pointer_t(&resp, &theme).max(f32::from(u8::from(open)));
        let corner = CornerRadius::same(radius::MD);
        let mut shapes = vec![Shape::rect_filled(rect, corner, mix(p.o1, p.o2, hover))];
        if let Some(o) = contrast_outline(rect, corner, &theme) {
            shapes.push(o);
        }
        let [pad_l, _] = control::SELECT_PAD;
        let cy = rect.center().y;
        let x = rect.left() + pad_l;
        let gw = g.size().x;
        shapes.push(Shape::galley(pos2(x, cy - g.size().y / 2.0), g, p.t1));
        shapes.push(glyph_shape(
            &painter,
            pos2(x + gw + layout::WORD_GAP + icon_size::SMALL / 2.0, cy),
            icons::CARET_DOWN,
            false,
            icon_size::SMALL,
            p.t3,
        ));
        painter.extend(shapes);
        if resp.has_focus() {
            focus_ring(&painter, rect, corner, &theme);
        }
    }
    let mut out = None;
    context_menu::popup(
        Popup::menu(&resp)
            .close_behavior(PopupCloseBehavior::CloseOnClick)
            .width(rect.width().max(layout::MENU_W[0])),
        |ui| {
            for (i, o) in c.options.iter().enumerate() {
                if context_menu::option_item(ui, o, c.chosen == Some(i)).clicked()
                    && c.chosen != Some(i)
                {
                    out = Some(Did::Chose(i));
                }
            }
            if let Some((label, why)) = &c.unavailable {
                context_menu::unavailable_item(ui, label, why);
            }
            if !c.actions.is_empty() {
                context_menu::separator(ui);
                for (label, act) in &c.actions {
                    if context_menu::item(ui, None, label, None, false).clicked() {
                        out = Some(Did::Act(*act));
                    }
                }
            }
        },
    );
    (resp, out)
}

/// One row (`.pr`): the icon tile, the name and its line, the control at the right. The
/// whole row toggles (`o1` under the pointer, `o2` pressed); the switch is the keyboard stop
/// and carries the name. Its ids are its own (`scope`), so a select coming and going beside
/// the switch, or a row above it, never moves the switch's focus.
fn prow(ui: &mut Ui, r: &Row, scope: Id) -> Option<Did> {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let [min_h, pad, reach] = gm::PROT_ROW;
    let [tile, glyph] = gm::PROT_TILE;
    let w = ui.available_width();
    let top = ui.cursor().min;
    let choice = r.choice.as_ref().filter(|_| r.on);
    let sw_w = if r.switch { control::TOGGLE_W } else { 0.0 };
    let choice_w = choice.map_or(0.0, |c| choice_width(ui, &c.short));
    let ctl_w = sw_w
        + choice_w
        + if sw_w > 0.0 && choice_w > 0.0 {
            gm::PROT_CONTROL_GAP
        } else {
            0.0
        };
    let text_x = top.x + tile + gm::PROT_GAP;
    let text_w = (w
        - tile
        - gm::PROT_GAP
        - if ctl_w > 0.0 {
            ctl_w + gm::PROT_GAP
        } else {
            0.0
        })
    .max(space::S16);
    let name = Type::BodyStrong.galley(ui, &r.name, p.t1, text_w, usize::MAX);
    let line = match &r.line {
        Line::Text(t) if t.is_empty() => None,
        Line::Text(t) => Some(rows::help_galley(ui, t, p.t3, text_w)),
        Line::Path(t) => Some(path_galley(ui, t, p.t3, text_w)),
    };
    let colour = |w: Weight| match w {
        Weight::Quiet => p.t3,
        Weight::Warning => p.amber,
        Weight::Danger => p.coral,
    };
    let extra: Vec<_> = r
        .extra
        .iter()
        .map(|(t, wt)| (rows::help_galley(ui, t, colour(*wt), text_w), colour(*wt)))
        .collect();
    let text_h = name.size().y
        + line.as_ref().map_or(0.0, |g| g.size().y)
        + extra.iter().map(|(g, _)| g.size().y).sum::<f32>();
    let h = (text_h + pad * 2.0).max(min_h);
    let rect = Rect::from_min_size(top, vec2(w, h));
    let plate = rect.expand2(vec2(reach, 0.0));
    // The plate is painted under what is laid out after it.
    let plate_at = ui.painter().add(Shape::Noop);
    // The whole row is a target, but not a keyboard stop: the switch is.
    let row_resp = r
        .switch
        .then(|| ui.interact(plate, scope.with("plate"), Sense::CLICK));
    let mut out = None;
    let mut sw_down = false;
    {
        let mut ctl = ui.new_child(
            UiBuilder::new()
                .scope_id(scope)
                .max_rect(rect)
                .layout(Layout::right_to_left(Align::Center)),
        );
        ctl.spacing_mut().item_spacing.x = gm::PROT_CONTROL_GAP;
        // What the control says after its name: the row's lines, and whether it is changed.
        let mut said: Vec<String> = Vec::new();
        match &r.line {
            Line::Text(t) | Line::Path(t) if !t.is_empty() => said.push(t.clone()),
            _ => {}
        }
        said.extend(r.extra.iter().map(|(t, _)| t.clone()));
        if r.changed {
            said.push(s::CHANGED_MARK().to_owned());
        }
        let description = said.iter().fold(String::new(), |mut all, x| {
            if !all.is_empty() {
                all.push_str(if all.ends_with('.') { " " } else { ". " });
            }
            all.push_str(x);
            all
        });
        if r.switch {
            let mut v = r.on;
            let resp = switch(&mut ctl, &mut v, &r.spoken);
            if resp.changed() {
                out = Some(Did::Switched(v));
            }
            // Delete takes a shared folder away, as switching it off does.
            if matches!(r.perm, Perm::UserDir(..))
                && r.on
                && resp.has_focus()
                && ctl.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
            {
                out = Some(Did::Switched(false));
            }
            sw_down = resp.is_pointer_button_down_on();
            if !description.is_empty() {
                crate::a11y::describe(&resp, &description);
            }
        }
        if let Some(c) = choice {
            let (resp, d) = pick(&mut ctl, c);
            if d.is_some() {
                out = d;
            }
            if !r.switch && !description.is_empty() {
                crate::a11y::describe(&resp, &description);
            }
        }
        if !r.switch && choice.is_none() {
            // Nothing to set: the row is read as its name, then what it says.
            let resp = ctl.interact(rect, scope.with("text"), Sense::hover());
            crate::a11y::item(&resp, egui::Role::Label, &r.name);
            if !description.is_empty() {
                crate::a11y::describe(&resp, &description);
            }
        }
    }
    if let Some(resp) = &row_resp
        && resp.clicked()
        && out.is_none()
    {
        out = Some(Did::Switched(!r.on));
    }
    // The plate: o1 under the pointer, o2 pressed; a row without a switch never highlights.
    if let Some(resp) = &row_resp {
        let hovered = ui.rect_contains_pointer(plate);
        let hover = ui.ctx().animate_bool_with_time(
            scope.with("hover"),
            hovered,
            theme.motion.fade(Dur::Fast),
        );
        let pressed = ui.ctx().animate_bool_with_time(
            scope.with("pressed"),
            resp.is_pointer_button_down_on() || sw_down,
            theme.motion.fade(Dur::Fast),
        );
        let fill = mix(mix(Color32::TRANSPARENT, p.o1, hover), p.o2, pressed);
        if fill.a() > 0 {
            ui.painter().set(
                plate_at,
                Shape::rect_filled(plate, CornerRadius::same(gm::PROT_ROW_RADIUS), fill),
            );
        }
    }
    // The tile: o1 and t3 while blocked, the accent tint and accent text while allowed.
    let lit =
        ui.ctx()
            .animate_bool_with_time(scope.with("lit"), r.on, theme.motion.fade(Dur::Base));
    let tile_rect =
        Rect::from_min_size(pos2(top.x, rect.center().y - tile / 2.0), vec2(tile, tile));
    let painter = ui.painter();
    painter.rect_filled(
        tile_rect,
        CornerRadius::same(gm::PROT_TILE_RADIUS),
        mix(p.o1, p.tint, lit),
    );
    painter.text(
        tile_rect.center(),
        Align2::CENTER_CENTER,
        r.icon.text(false),
        icons::font(glyph),
        mix(p.t3, p.accent_text, lit),
    );
    // The name, its line, and any third lines, centred in the row.
    let mut y = rect.center().y - text_h / 2.0;
    let nh = name.size().y;
    painter.galley(pos2(text_x, y), name, p.t1);
    y += nh;
    if let Some(g) = line {
        let gh = g.size().y;
        painter.galley(pos2(text_x, y), g, p.t3);
        y += gh;
    }
    for (g, c) in extra {
        let gh = g.size().y;
        painter.galley(pos2(text_x, y), g, c);
        y += gh;
    }
    ui.allocate_rect(rect, Sense::hover());
    out
}

/// A group's heading (`.h.ph`): the Overview's 15 pt heading, its count at the right in 12
/// t3 on the same baseline; 32 above (none for the first), 4 below.
fn group_heading(ui: &mut Ui, title: &str, count: Option<&str>, first: bool) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if !first {
        ui.add_space(layout::HEADING_GAP[0]);
    }
    let h = Type::Heading.line_height()
        + if count.is_some() {
            gm::PROT_COUNT_DEPTH
        } else {
            0.0
        };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let t = Type::Heading.line(ui, title, p.t1);
    let baseline = |g: &Galley| {
        g.rows
            .first()
            .and_then(|r| r.row.glyphs.first())
            .map_or(g.size().y, |gl| gl.pos.y)
    };
    let title_base = baseline(&t);
    ui.painter().galley(rect.min, t, p.t1);
    if let Some(count) = count {
        let g = Type::Caption.line(ui, count, p.t3);
        let at = pos2(
            rect.right() - g.size().x,
            rect.top() + title_base - baseline(&g),
        );
        ui.painter().galley(at, g, p.t3);
        crate::a11y::describe(&resp, count);
    }
    crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Heading, title);
    ui.add_space(gm::PROT_HEADING);
}

/// A text button under the list (`.b`): 30 high, its words 500 in `t2` (`t1` on `o2` under
/// the pointer), a 13 pt glyph `gap` before them, hanging 8 left of the column. `turn`
/// turns the glyph a quarter (0 to 1, eased); `open` drops the plate (`.disc` open).
fn text_button(
    ui: &mut Ui,
    id: Id,
    icon: Icon,
    label: &str,
    gap: f32,
    turn: f32,
    open: bool,
) -> Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let painter = ui.painter().clone();
    let g = label_galley(
        &painter,
        label,
        Type::BodyStrong.font_id(),
        Type::Body.line_height(),
    );
    let pad = control::BUTTON_PAD;
    let w = pad * 2.0 + icon_size::SMALL + gap + g.size().x;
    let at = ui.cursor().min;
    let (_, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), control::BUTTON), Sense::hover());
    let rect = Rect::from_min_size(pos2(at.x - gm::PROT_HANG, at.y), vec2(w, control::BUTTON));
    let resp = ui.interact(rect, id, Sense::click());
    crate::a11y::button(&resp, label);
    let hover = crate::widgets::hover_t(&resp, &theme);
    let pressed = crate::widgets::press_t(&resp, &theme);
    let corner = CornerRadius::same(radius::MD);
    if !open {
        let fill = mix(mix(Color32::TRANSPARENT, p.o2, hover), p.o3, pressed);
        painter.rect_filled(rect, corner, fill);
    }
    let c = mix(p.t2, p.t1, hover);
    let cy = rect.center().y;
    let center = pos2(rect.left() + pad + icon_size::SMALL / 2.0, cy);
    let glyph = painter.layout_no_wrap(
        icon.text(false).to_owned(),
        icons::font(icon_size::SMALL),
        c,
    );
    let half = glyph.size() / 2.0;
    painter.add(
        egui::epaint::TextShape::new(center - half, glyph, c)
            .with_angle_and_anchor(turn * std::f32::consts::FRAC_PI_2, Align2::CENTER_CENTER),
    );
    painter.galley(
        pos2(
            rect.left() + pad + icon_size::SMALL + gap,
            cy - g.size().y / 2.0,
        ),
        g,
        c,
    );
    if resp.has_focus() {
        focus_ring(&painter, rect, corner, &theme);
    }
    resp
}

// ---- Deciding ------------------------------------------------------------------------------

/// Put what the user did to the row of `perm` into the draft.
fn decide(state: &mut UiState, env: &mut Env<'_>, game: GameId, perm: Perm, did: Did) {
    let now = env.now;
    let Some(e) = editor(state) else { return };
    let mut pick = None;
    let (was, mut granted) = (e.saved.permissions(), e.draft.permissions());
    let external = matches!(e.draft.save_data, SaveDataLocation::External { .. });
    match (perm, did) {
        (Perm::Flag(bit), Did::Switched(on)) => {
            granted.set(bit, on);
            // A permission with degrees comes back on at the degree that is saved; a degree
            // switched on brings the permission it widens.
            if on {
                granted |= bit.prerequisites() | (was & bit.degrees());
            }
            e.draft.set_permissions(granted);
        }
        (Perm::Flag(bit), Did::Chose(k)) => {
            granted.set(bit.degrees(), k == 1);
            e.draft.set_permissions(granted);
        }
        (Perm::GameFolder, Did::Chose(k)) => {
            e.draft.game_dir = [
                GameDirAccess::ReadWrite,
                GameDirAccess::ReadOnly,
                GameDirAccess::Protected,
            ][k.min(2)];
        }
        (Perm::SaveData, Did::Chose(0)) => e.draft.save_data = SaveDataLocation::Internal,
        // The folder already chosen, chosen again.
        (Perm::SaveData, Did::Chose(1)) if external => return,
        (Perm::SaveData, Did::Chose(_)) => pick = Some(PickPurpose::SaveFolder(Some(game))),
        (Perm::UserDir(d, path), Did::Switched(true)) => {
            // A folder the saved policy shares comes back as it was saved, at its place.
            match e.saved.folders.iter().position(|f| is_dir(f, d, &path)) {
                Some(j) => {
                    if let Some(f) = e.saved.folders.get(j).cloned() {
                        let at = j.min(e.draft.folders.len());
                        e.draft.folders.insert(at, f);
                    }
                }
                None => e.toggle_user_dir(d, path, now),
            }
        }
        (Perm::UserDir(d, path), Did::Switched(false)) => {
            e.draft.folders.retain(|f| !is_dir(f, d, &path));
        }
        (Perm::Shared(path), Did::Chose(k)) => {
            if let Some(f) = e.draft.folders.iter_mut().find(|f| f.canonical == path) {
                f.access = if k == 1 {
                    FolderAccess::ReadWrite
                } else {
                    FolderAccess::ReadOnly
                };
            }
        }
        (Perm::Shared(path), Did::Act(Act::Locate)) => {
            pick = Some(PickPurpose::RelocateFolder(game, path));
        }
        (Perm::Shared(path), Did::Act(Act::StopSharing)) => {
            e.draft.folders.retain(|f| f.canonical != path);
        }
        _ => return,
    }
    e.reclassify();
    e.folder_error = None;
    state.picks.extend(pick);
    reassess(state, env);
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

/// What Advanced holds, as setting rows: how far the home folders and removable drives may
/// go, identity, the system-call filter, the layer's copy, direct sound, and the screen. A
/// switch that protects is on while its permission is withheld.
fn advanced(ui: &mut Ui, e: &mut PolicyEditor, env: &Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let before = e.draft.permissions();
    let mut granted = before;
    let mut changed = false;

    // How far each home folder that is allowed, and removable drives, may go.
    let dirs = home_dirs(env);
    let allowed: Vec<(usize, String)> = dirs
        .iter()
        .filter_map(|(d, path)| {
            e.draft
                .folders
                .iter()
                .position(|f| is_dir(f, *d, path))
                .map(|i| (i, s::folder_named(s::user_dir(*d))))
        })
        .collect();
    let removable = granted.contains(Permissions::REMOVABLE);
    let mut first = true;
    if !allowed.is_empty() || removable {
        rows::heading(ui, s::CHANGING_FILES(), true);
        super::fine(ui, s::CHANGING_FILES_HELP(), p.t3);
        first = false;
        for (i, name) in &allowed {
            let Some(f) = e.draft.folders.get_mut(*i) else {
                continue;
            };
            let mut on = f.access == FolderAccess::ReadWrite;
            let spoken = s::can_change_name(name);
            if SettingRow::new(name)
                .id_salt(("prot.change", name))
                .show(ui, |ui| switch(ui, &mut on, &spoken))
                .changed()
            {
                f.access = if on {
                    FolderAccess::ReadWrite
                } else {
                    FolderAccess::ReadOnly
                };
                changed = true;
            }
        }
        if removable {
            let mut on = granted.contains(Permissions::REMOVABLE_WRITE);
            let spoken = s::can_change_name(s::REMOVABLE());
            if SettingRow::new(s::REMOVABLE())
                .show(ui, |ui| switch(ui, &mut on, &spoken))
                .changed()
            {
                granted.set(Permissions::REMOVABLE_WRITE, on);
            }
        }
    }

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
    rows::heading(ui, s::IDENTITY(), first);
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
    if SettingRow::new(s::LAYER_ISOLATION())
        .show(ui, |ui| {
            segmented(
                ui,
                s::LAYER_ISOLATION(),
                &mut iso,
                &segs(&[s::ISO_AUTO(), s::ISO_PRIVATE(), s::ISO_SHARED()]),
                false,
            )
        })
        .changed()
    {
        h.runtime_isolation = [
            RuntimeIsolation::Auto,
            RuntimeIsolation::Isolated,
            RuntimeIsolation::Shared,
        ][iso.min(2)];
        changed = true;
    }
    let mut direct = granted.contains(Permissions::DIRECT_SOUND);
    if SettingRow::new(s::DIRECT_SOUND())
        .help(s::DIRECT_SOUND_HINT())
        .show(ui, |ui| switch(ui, &mut direct, s::DIRECT_SOUND()))
        .changed()
    {
        granted.set(Permissions::DIRECT_SOUND, direct);
    }

    // Where the game draws cannot be decided here; what it means is said.
    let lines = screen_lines(e, env);
    if !lines.is_empty() {
        let colour = |w: Weight| match w {
            Weight::Quiet => p.t3,
            Weight::Warning => p.amber,
            Weight::Danger => p.coral,
        };
        SettingRow::new(s::SCREEN()).show_with(
            ui,
            |ui| {
                let mut said = None;
                for (text, weight) in &lines {
                    let w = ui.available_width().min(layout::HELP_MAX_W);
                    let g = rows::help_galley(ui, text, colour(*weight), w);
                    let (rect, resp) = ui.allocate_exact_size(g.size(), Sense::hover());
                    ui.painter().galley(rect.min, g, colour(*weight));
                    said.get_or_insert(resp);
                }
                if let Some(resp) = said {
                    let all: Vec<&str> = lines.iter().map(|(t, _)| t.as_str()).collect();
                    crate::a11y::item(&resp, egui::Role::Label, s::SCREEN());
                    crate::a11y::describe(&resp, &all.join(" "));
                }
            },
            |_| {},
        );
    }

    if granted != before {
        e.draft.set_permissions(granted);
        changed = true;
    }
    if changed {
        e.reclassify();
    }
}

/// What the screen means for the game: the display statements, and the X11 notice.
fn screen_lines(e: &PolicyEditor, env: &Env<'_>) -> Vec<(String, Weight)> {
    let mut out = Vec::new();
    let mut x11 = false;
    for st in &e.assessment.statements {
        if st.category == AccessCategory::Display {
            x11 |= st.key.contains("x11");
            let weight = match st.severity {
                StatementSeverity::Warning => Weight::Warning,
                StatementSeverity::Danger => Weight::Danger,
                _ => Weight::Quiet,
            };
            out.push((st.text.clone(), weight));
        }
    }
    if env.bridge.host().x11 && !x11 {
        out.push((s::X11_NOTICE().to_owned(), Weight::Warning));
    }
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
    reassess(state, env);
    let Some(e) = editor(state) else { return };
    let groups = [
        (s::INTERNET_AND_DEVICES(), devices(e, env)),
        (s::OWN_FILES(), own(e, g)),
        (s::YOUR_FILES(), yours(e, env)),
    ];
    let allowed = |rows: &[Row]| rows.iter().filter(|r| r.switch && r.on).count();
    let shared = |rows: &[Row]| {
        rows.iter()
            .filter(|r| (r.on && r.switch) || matches!(r.perm, Perm::Shared(_)))
            .count()
    };
    let counts = [
        Some(s::devices_allowed(allowed(&groups[0].1), groups[0].1.len())),
        None,
        Some(s::folders_shared(shared(&groups[2].1))),
    ];
    let rejected = e.folder_error.as_deref().map(s::rejected);
    let running = g.activity != zaun_core::model::Activity::Idle;
    let p = theme::theme(ui.ctx()).palette.clone();
    let mut done = None;
    let mut add = false;
    let mut disclose = false;
    super::column(ui, |ui| {
        Type::Lead.paragraph(ui, s::PROT_LEDE(), p.t2);
        ui.add_space(gm::PROT_LEDE_GAP);
        let list_top = ui.cursor().min;
        for (i, ((title, rows), count)) in groups.iter().zip(&counts).enumerate() {
            group_heading(ui, title, count.as_deref(), i == 0);
            for r in rows {
                let scope = Id::unique(("protection.row", g.id, &r.key));
                if let Some(did) = prow(ui, r, scope) {
                    done = Some((r.perm.clone(), did));
                }
            }
        }
        // "+ Share another folder…", and where a folder can be dropped.
        ui.add_space(gm::PROT_ADD[0]);
        let b = text_button(
            ui,
            Id::unique(("protection.add", g.id)),
            icons::PLUS,
            s::SHARE_ANOTHER(),
            gm::PROT_ADD[1],
            0.0,
            false,
        );
        add = crate::widgets::tooltip::tooltip(b, s::DROP_FOLDER(), None).clicked();
        let list = Rect::from_min_max(
            list_top,
            pos2(list_top.x + ui.available_width(), ui.cursor().min.y),
        );
        state.dnd.target(list, DropTarget::FolderList(Some(g.id)));
        // A folder that was refused is explained where it was asked for.
        if let Some(rejected) = &rejected {
            ui.add_space(gm::PROT_ADD[0]);
            issue_line(ui, rejected, true);
        }
        if running {
            ui.add_space(gm::PROT_FINE_TOP);
            super::fine(ui, s::APPLIES_NEXT_TIME(), p.t3);
        }

        // Advanced.
        let Some(e) = editor(state) else { return };
        ui.add_space(gm::PROT_DISC[0]);
        let open = e.advanced;
        let theme = theme::theme(ui.ctx());
        let turn = theme::motion::glide(
            ui.ctx(),
            Id::unique(("protection.advanced.turn", g.id)),
            f32::from(u8::from(open)),
            theme.motion.movement(Dur::Base),
        );
        let n = s::advanced_changes(&e.saved, &e.draft);
        let label = s::advanced(n);
        let r = text_button(
            ui,
            Id::unique(("protection.advanced", g.id)),
            icons::CARET_RIGHT,
            &label,
            gm::PROT_DISC[1],
            turn,
            open,
        );
        ui.ctx()
            .accesskit_node_builder(r.id, |n| n.set_expanded(open));
        disclose = r.clicked();
        if open {
            ui.add_space(gm::PROT_DISC[2]);
            ui.scope_builder(
                UiBuilder::new().scope_id(Id::unique(("protection.advanced.rows", g.id))),
                |ui| advanced(ui, e, env),
            );
        } else {
            // Closed, Advanced still says what it has opened up; each line opens it.
            let mut lines: Vec<(String, Color32)> = Vec::new();
            for st in &e.assessment.statements {
                let shown_in_rows = matches!(
                    st.category,
                    AccessCategory::Network
                        | AccessCategory::OwnFolder
                        | AccessCategory::YourFolders
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
                lines.push((st.text.clone(), c));
            }
            if env.bridge.host().x11
                && !e
                    .assessment
                    .statements
                    .iter()
                    .any(|st| st.category == AccessCategory::Display && st.key.contains("x11"))
            {
                lines.push((s::X11_NOTICE().to_owned(), p.amber));
            }
            for (text, c) in &lines {
                let r = super::fine(ui, text, *c).interact(Sense::click());
                crate::a11y::button(&r, text);
                if r.hovered() || r.has_focus() {
                    ui.painter().hline(
                        r.rect.x_range(),
                        r.rect.bottom() - stroke::BORDER,
                        egui::Stroke::new(stroke::BORDER, *c),
                    );
                }
                disclose |= r.clicked();
            }
        }
    });
    if disclose && let Some(e) = editor(state) {
        e.advanced = !e.advanced;
    }
    match done {
        Some((perm, did)) => decide(state, env, g.id, perm, did),
        None if add => state.picks.push(PickPurpose::FolderGrant(Some(g.id))),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_stays_left_to_right_in_a_right_to_left_interface() {
        let path = "/home/sam/\u{0627}\u{0644}\u{0635}\u{0648}\u{0631}/Mods";
        // Under the interface's own (right-to-left) base the separators would move.
        let rtl = bidi::visual_line(path, Some(crate::i18n::Dir::Rtl)).into_owned();
        assert!(!rtl.starts_with("/home/sam/"), "{rtl}");
        let shown = path_text(path);
        assert!(shown.starts_with("/home/sam/"), "{shown}");
        assert!(shown.ends_with("/Mods"), "{shown}");
        assert_eq!(path_text("/home/sam/Pictures"), "/home/sam/Pictures");
    }
}
