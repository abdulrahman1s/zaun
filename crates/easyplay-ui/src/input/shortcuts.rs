//! The shortcut table of USER_EXPERIENCE §5.2 as data, its scopes, the text-field conflict
//! rule and dispatch. The in-app sheet (`?`, `F1`) is generated from [`TABLE`].
//!
//! Keys handled inside a focused composite widget (arrow keys and `Enter`/`Space` on grid
//! cards, tab lists, menus) are listed for the sheet with [`Binding::local`] and are not
//! dispatched here.

use egui::{Context, Key, Modifiers};

use crate::strings::shortcuts as s;

/// Where a shortcut applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Everywhere (no overlay open).
    Global,
    /// The library route.
    Library,
    /// The game page.
    GamePage,
    /// The game page and settings (tab navigation, back).
    Pages,
    /// Library and game page.
    Games,
}

impl Scope {
    /// Whether this scope is active on `here`.
    pub fn applies(self, here: Here) -> bool {
        match self {
            Scope::Global => true,
            Scope::Library => here == Here::Library,
            Scope::GamePage => here == Here::GamePage,
            Scope::Pages => matches!(here, Here::GamePage | Here::Settings | Here::Other),
            Scope::Games => matches!(here, Here::Library | Here::GamePage),
        }
    }

    /// Whether two scopes can be active at once.
    pub fn overlaps(self, other: Scope) -> bool {
        [Here::Library, Here::GamePage, Here::Settings, Here::Other]
            .into_iter()
            .any(|h| self.applies(h) && other.applies(h))
    }
}

/// The current place, for scope checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Here {
    /// Library.
    Library,
    /// Game page.
    GamePage,
    /// Settings.
    Settings,
    /// Statistics and anything else.
    Other,
}

/// What a shortcut does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Focus the search field.
    FocusSearch,
    /// Search the whole library.
    SearchAll,
    /// Add game (file picker).
    AddGame,
    /// Open the selected game's page (local to the grid).
    Open,
    /// Play the selected game.
    Play,
    /// Stop the selected game.
    Stop,
    /// Toggle favourite.
    Favorite,
    /// Edit tags.
    Tags,
    /// Hide or unhide.
    Hide,
    /// Remove (with confirmation).
    Remove,
    /// Rename.
    Rename,
    /// Grid view.
    GridView,
    /// List view.
    ListView,
    /// Toggle the sidebar.
    ToggleSidebar,
    /// Open settings.
    Settings,
    /// Back.
    Back,
    /// Next tab.
    NextTab,
    /// Previous tab.
    PrevTab,
    /// Select all in view.
    SelectAll,
    /// Undo the last undoable action.
    Undo,
    /// Context menu.
    ContextMenu,
    /// Larger UI.
    ZoomIn,
    /// Smaller UI.
    ZoomOut,
    /// Reset UI scale.
    ZoomReset,
    /// Re-check missing games.
    Refresh,
    /// Fullscreen.
    Fullscreen,
    /// Shortcut sheet.
    ShortcutSheet,
    /// Quit.
    Quit,
    /// Open the game folder.
    OpenFolder,
}

/// One key combination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Combo {
    /// Modifiers (`COMMAND` = Ctrl).
    pub mods: Modifiers,
    /// Key.
    pub key: Key,
}

const fn c(mods: Modifiers, key: Key) -> Combo {
    Combo { mods, key }
}

const NONE: Modifiers = Modifiers::NONE;
const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers {
    alt: false,
    ctrl: false,
    shift: true,
    mac_cmd: false,
    command: true,
};
const SHIFT: Modifiers = Modifiers::SHIFT;
const ALT: Modifiers = Modifiers::ALT;

impl Combo {
    /// Whether text editing could need this combination (plain keys, `Ctrl+A`, `Ctrl+Z`,
    /// arrows, `Backspace`, `Delete`, `Home`/`End`, `Enter`, `Space`).
    pub fn conflicts_with_text(self) -> bool {
        let plain = !self.mods.command && !self.mods.alt && !self.mods.ctrl;
        plain
            || (self.mods.command
                && matches!(
                    self.key,
                    Key::A
                        | Key::Z
                        | Key::C
                        | Key::V
                        | Key::X
                        | Key::Y
                        | Key::Backspace
                        | Key::Delete
                        | Key::ArrowLeft
                        | Key::ArrowRight
                ))
    }

    /// Whether this is a single key without `Ctrl` or `Alt` (needs the preference).
    pub fn is_single_key(self) -> bool {
        !self.mods.command && !self.mods.alt && !self.mods.ctrl
    }

    /// "Ctrl+Shift+F".
    pub fn label(self) -> String {
        let mut out = String::new();
        if self.mods.command || self.mods.ctrl {
            out.push_str(s::CTRL);
            out.push('+');
        }
        if self.mods.alt {
            out.push_str(s::ALT);
            out.push('+');
        }
        if self.mods.shift {
            out.push_str(s::SHIFT);
            out.push('+');
        }
        out.push_str(&key_name(self.key));
        out
    }
}

fn key_name(k: Key) -> String {
    match k {
        Key::ArrowLeft => "←".into(),
        Key::ArrowRight => "→".into(),
        Key::Enter => s::ENTER.into(),
        Key::Space => s::SPACE.into(),
        Key::Escape => s::ESC.into(),
        Key::Delete => s::DELETE.into(),
        Key::Backspace => s::BACKSPACE.into(),
        Key::Tab => s::TAB.into(),
        Key::PageDown => s::PAGE_DOWN.into(),
        Key::PageUp => s::PAGE_UP.into(),
        Key::Comma => ",".into(),
        Key::Period => ".".into(),
        Key::Slash => "/".into(),
        Key::Questionmark => "?".into(),
        Key::Plus => s::PLUS.into(),
        Key::Equals => "=".into(),
        Key::Minus => "-".into(),
        Key::Num0 => "0".into(),
        Key::Num1 => "1".into(),
        Key::Num2 => "2".into(),
        other => other.name().to_owned(),
    }
}

/// One row of the table.
#[derive(Clone, Copy, Debug)]
pub struct Binding {
    /// Combinations (any triggers it).
    pub keys: &'static [Combo],
    /// Action.
    pub action: Action,
    /// Scope.
    pub scope: Scope,
    /// Sheet description.
    pub label: &'static str,
    /// Handled by a focused widget, not by [`dispatch`].
    pub local: bool,
}

const fn b(keys: &'static [Combo], action: Action, scope: Scope, label: &'static str) -> Binding {
    Binding {
        keys,
        action,
        scope,
        label,
        local: false,
    }
}

const fn local(
    keys: &'static [Combo],
    action: Action,
    scope: Scope,
    label: &'static str,
) -> Binding {
    Binding {
        keys,
        action,
        scope,
        label,
        local: true,
    }
}

/// USER_EXPERIENCE §5.2.
pub const TABLE: &[Binding] = &[
    b(
        &[c(CTRL, Key::F), c(NONE, Key::Slash)],
        Action::FocusSearch,
        Scope::Library,
        s::FOCUS_SEARCH,
    ),
    b(
        &[c(CTRL_SHIFT, Key::F)],
        Action::SearchAll,
        Scope::Library,
        s::SEARCH_ALL,
    ),
    b(
        &[c(CTRL, Key::N), c(CTRL, Key::O)],
        Action::AddGame,
        Scope::Library,
        s::ADD_GAME,
    ),
    local(
        &[c(NONE, Key::Enter)],
        Action::Open,
        Scope::Library,
        s::OPEN,
    ),
    b(&[c(CTRL, Key::Enter)], Action::Play, Scope::Games, s::PLAY),
    local(
        &[c(NONE, Key::Space)],
        Action::Play,
        Scope::Library,
        s::PLAY_CARD,
    ),
    b(&[c(CTRL, Key::Period)], Action::Stop, Scope::Games, s::STOP),
    b(
        &[c(NONE, Key::F)],
        Action::Favorite,
        Scope::Games,
        s::FAVORITE,
    ),
    b(&[c(NONE, Key::T)], Action::Tags, Scope::Games, s::TAGS),
    b(&[c(NONE, Key::H)], Action::Hide, Scope::Library, s::HIDE),
    b(
        &[c(NONE, Key::Delete)],
        Action::Remove,
        Scope::Library,
        s::REMOVE,
    ),
    b(
        &[c(NONE, Key::F2)],
        Action::Rename,
        Scope::GamePage,
        s::RENAME,
    ),
    b(
        &[c(CTRL, Key::Num1)],
        Action::GridView,
        Scope::Library,
        s::GRID,
    ),
    b(
        &[c(CTRL, Key::Num2)],
        Action::ListView,
        Scope::Library,
        s::LIST,
    ),
    b(
        &[c(CTRL, Key::B)],
        Action::ToggleSidebar,
        Scope::Global,
        s::SIDEBAR,
    ),
    b(
        &[c(CTRL, Key::Comma)],
        Action::Settings,
        Scope::Global,
        s::SETTINGS,
    ),
    b(
        &[
            c(ALT, Key::ArrowLeft),
            c(NONE, Key::Escape),
            c(NONE, Key::Backspace),
        ],
        Action::Back,
        Scope::Pages,
        s::BACK,
    ),
    b(
        &[c(CTRL, Key::Tab), c(CTRL, Key::PageDown)],
        Action::NextTab,
        Scope::Pages,
        s::NEXT_TAB,
    ),
    b(
        &[c(CTRL_SHIFT, Key::Tab), c(CTRL, Key::PageUp)],
        Action::PrevTab,
        Scope::Pages,
        s::PREV_TAB,
    ),
    b(
        &[c(CTRL, Key::A)],
        Action::SelectAll,
        Scope::Library,
        s::SELECT_ALL,
    ),
    b(&[c(CTRL, Key::Z)], Action::Undo, Scope::Global, s::UNDO),
    b(
        &[c(SHIFT, Key::F10)],
        Action::ContextMenu,
        Scope::Library,
        s::CONTEXT_MENU,
    ),
    local(
        &[c(CTRL, Key::Plus), c(CTRL, Key::Equals)],
        Action::ZoomIn,
        Scope::Global,
        s::ZOOM_IN,
    ),
    local(
        &[c(CTRL, Key::Minus)],
        Action::ZoomOut,
        Scope::Global,
        s::ZOOM_OUT,
    ),
    local(
        &[c(CTRL, Key::Num0)],
        Action::ZoomReset,
        Scope::Global,
        s::ZOOM_RESET,
    ),
    b(
        &[c(NONE, Key::F5)],
        Action::Refresh,
        Scope::Library,
        s::REFRESH,
    ),
    b(
        &[c(NONE, Key::F11)],
        Action::Fullscreen,
        Scope::Global,
        s::FULLSCREEN,
    ),
    b(
        &[
            c(NONE, Key::Questionmark),
            c(SHIFT, Key::Slash),
            c(NONE, Key::F1),
        ],
        Action::ShortcutSheet,
        Scope::Global,
        s::SHEET,
    ),
    b(&[c(CTRL, Key::Q)], Action::Quit, Scope::Global, s::QUIT),
    b(
        &[c(CTRL, Key::O)],
        Action::OpenFolder,
        Scope::GamePage,
        s::OPEN_FOLDER,
    ),
];

/// The context of one dispatch.
#[derive(Clone, Copy, Debug)]
pub struct DispatchCtx {
    /// Where the user is.
    pub here: Here,
    /// A text field has focus.
    pub text_focus: bool,
    /// `integration.single_key_shortcuts`.
    pub single_keys: bool,
}

/// Whether `combo` may fire in `ctx`.
pub fn allowed(combo: Combo, ctx: DispatchCtx) -> bool {
    if ctx.text_focus && combo.conflicts_with_text() {
        // `Esc` and `F`-keys still work from a field.
        return matches!(combo.key, Key::F1 | Key::F2 | Key::F5 | Key::F10 | Key::F11)
            && combo.is_single_key()
            && combo.key != Key::F2;
    }
    if combo.is_single_key() && !ctx.single_keys {
        return matches!(
            combo.key,
            Key::Escape | Key::F1 | Key::F2 | Key::F5 | Key::F11 | Key::Delete | Key::Backspace
        );
    }
    true
}

/// Consume every shortcut pressed this frame that applies here; returns the actions.
pub fn dispatch(ctx: &Context, d: DispatchCtx) -> Vec<Action> {
    let mut out = Vec::new();
    ctx.input_mut(|i| {
        for bind in TABLE {
            if bind.local || !bind.scope.applies(d.here) {
                continue;
            }
            for combo in bind.keys {
                if !allowed(*combo, d) {
                    continue;
                }
                if i.consume_key(combo.mods, combo.key) {
                    out.push(bind.action);
                    break;
                }
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No two shortcuts conflict within a scope (UI-030 acceptance).
    #[test]
    fn no_conflicts_within_a_scope() {
        let mut seen: Vec<(Combo, Scope, Action)> = Vec::new();
        for bind in TABLE {
            for combo in bind.keys {
                for (other, scope, action) in &seen {
                    if other == combo && scope.overlaps(bind.scope) {
                        // The same key may stand for the same action in two scopes.
                        assert_eq!(
                            *action,
                            bind.action,
                            "{} is bound twice: {:?} and {:?}",
                            combo.label(),
                            action,
                            bind.action
                        );
                    }
                }
                seen.push((*combo, bind.scope, bind.action));
            }
        }
    }

    #[test]
    fn text_fields_keep_their_keys() {
        let d = DispatchCtx {
            here: Here::Library,
            text_focus: true,
            single_keys: true,
        };
        assert!(!allowed(c(NONE, Key::F), d));
        assert!(!allowed(c(CTRL, Key::A), d));
        assert!(!allowed(c(NONE, Key::Slash), d));
        assert!(allowed(c(CTRL, Key::B), d));
        assert!(allowed(c(CTRL, Key::Comma), d));
        assert!(allowed(c(NONE, Key::F5), d));
    }

    #[test]
    fn single_keys_follow_the_preference() {
        let d = DispatchCtx {
            here: Here::Library,
            text_focus: false,
            single_keys: false,
        };
        assert!(!allowed(c(NONE, Key::F), d));
        assert!(allowed(c(NONE, Key::Delete), d));
        assert!(allowed(c(CTRL, Key::F), d));
    }

    #[test]
    fn labels() {
        assert_eq!(c(CTRL_SHIFT, Key::F).label(), "Ctrl+Shift+F");
        assert_eq!(c(ALT, Key::ArrowLeft).label(), "Alt+←");
    }
}
