//! `Route { Library, Game(GameId), Statistics, Settings(SettingsTab), Onboarding }`, the back
//! stack and the overlay stack. See MODULES §8, USER_EXPERIENCE §2 and task UI-001.

use easyplay_core::ids::GameId;

use crate::strings;

/// Settings tabs (USER_EXPERIENCE §2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SettingsTab {
    /// General.
    #[default]
    General,
    /// Appearance.
    Appearance,
    /// Protection defaults.
    Protection,
    /// Compatibility layers.
    CompatibilityLayers,
    /// Desktop integration.
    Integration,
    /// Advanced.
    Advanced,
    /// About.
    About,
}

/// A full-window destination.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Route {
    /// The library (default).
    #[default]
    Library,
    /// One game's page.
    Game(GameId),
    /// Statistics.
    Statistics,
    /// Settings.
    Settings(SettingsTab),
    /// First launch.
    Onboarding,
    /// Debug-only widget gallery (UI-004).
    Gallery,
}

impl Route {
    /// The name used in the window title (the game's name is supplied by the caller).
    pub fn title<'a>(&self, game_name: Option<&'a str>) -> &'a str {
        match self {
            Route::Library => strings::library::TITLE,
            Route::Game(_) => game_name.unwrap_or(strings::library::TITLE),
            Route::Statistics => strings::nav::STATISTICS,
            Route::Settings(_) => strings::nav::SETTINGS,
            Route::Onboarding => strings::nav::ONBOARDING,
            Route::Gallery => strings::nav::GALLERY,
        }
    }

    /// Whether the route shows the sidebar.
    pub const fn has_sidebar(&self) -> bool {
        !matches!(self, Route::Onboarding)
    }
}

/// A dialog awaiting a decision; the id keys the dialog queue (`vm::dialogs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DialogId(pub u32);

/// A modal layer above any route.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Overlay {
    /// First-time setup for a program (sheet).
    Setup,
    /// Add game (sheet).
    AddGame,
    /// Launch inspector, Developer Mode (sheet).
    Inspector,
    /// Keyboard shortcut sheet.
    Shortcuts,
    /// Artwork sheet for a game.
    Artwork(GameId),
    /// Tag editor for a game.
    Tags(GameId),
    /// Game-fix picker for a game.
    FixPicker(GameId),
    /// Full play history (statistics).
    History,
    /// A dialog; may sit above a sheet.
    Dialog(DialogId),
}

impl Overlay {
    /// Sheets occupy the lower layer; dialogs may stack above one.
    pub const fn is_sheet(&self) -> bool {
        !matches!(self, Overlay::Dialog(_))
    }
}

/// The back stack and overlay stack. Never more than two overlay layers (USER_EXPERIENCE §2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Navigator {
    stack: Vec<Route>,
    overlays: Vec<Overlay>,
}

impl Default for Navigator {
    fn default() -> Self {
        Self::new(Route::Library)
    }
}

impl Navigator {
    /// Start at `root`.
    pub fn new(root: Route) -> Self {
        Self {
            stack: vec![root],
            overlays: Vec::new(),
        }
    }

    /// The current route.
    pub fn current(&self) -> Route {
        self.stack.last().copied().unwrap_or_default()
    }

    /// The overlays, bottom first.
    pub fn overlays(&self) -> &[Overlay] {
        &self.overlays
    }

    /// The topmost overlay.
    pub fn top_overlay(&self) -> Option<Overlay> {
        self.overlays.last().copied()
    }

    /// Whether a back step is possible (an overlay or a deeper route).
    pub fn can_go_back(&self) -> bool {
        !self.overlays.is_empty() || self.stack.len() > 1
    }

    /// Go deeper (library → game page). Pushing the current route is a no-op.
    pub fn push(&mut self, route: Route) {
        if self.current() != route {
            self.stack.push(route);
        }
    }

    /// Switch to a top-level destination from the sidebar: the stack becomes `[route]` and
    /// overlays close.
    pub fn go_to(&mut self, route: Route) {
        self.stack.clear();
        self.stack.push(route);
        self.overlays.clear();
    }

    /// Replace the current route (switching settings tabs).
    pub fn replace(&mut self, route: Route) {
        if let Some(top) = self.stack.last_mut() {
            *top = route;
        }
    }

    /// Back: close the topmost overlay, else pop a route. Returns whether anything changed.
    pub fn back(&mut self) -> bool {
        if self.overlays.pop().is_some() {
            return true;
        }
        if self.stack.len() > 1 {
            self.stack.pop();
            return true;
        }
        false
    }

    /// Open an overlay. A sheet replaces any open sheet (and the dialog above it); a dialog
    /// replaces an open dialog. The result never exceeds one sheet plus one dialog.
    pub fn open(&mut self, overlay: Overlay) {
        if overlay.is_sheet() {
            self.overlays.clear();
        } else if self.top_overlay().is_some_and(|o| !o.is_sheet()) {
            self.overlays.pop();
        }
        self.overlays.push(overlay);
    }

    /// Close a specific overlay (and anything above it).
    pub fn close(&mut self, overlay: Overlay) {
        if let Some(i) = self.overlays.iter().position(|o| *o == overlay) {
            self.overlays.truncate(i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> GameId {
        GameId::from_uuid(uuid::Uuid::from_u128(7))
    }

    #[test]
    fn push_and_back() {
        let mut n = Navigator::default();
        assert!(!n.can_go_back());
        n.push(Route::Game(game()));
        n.push(Route::Game(game()));
        assert_eq!(n.current(), Route::Game(game()));
        assert!(n.back());
        assert_eq!(n.current(), Route::Library);
        assert!(!n.back());
    }

    #[test]
    fn overlays_close_before_routes() {
        let mut n = Navigator::default();
        n.push(Route::Game(game()));
        n.open(Overlay::Setup);
        n.open(Overlay::Dialog(DialogId(1)));
        assert_eq!(n.overlays().len(), 2);
        assert!(n.back());
        assert_eq!(n.top_overlay(), Some(Overlay::Setup));
        assert!(n.back());
        assert_eq!(n.top_overlay(), None);
        assert_eq!(n.current(), Route::Game(game()));
    }

    #[test]
    fn never_more_than_two_layers() {
        let mut n = Navigator::default();
        n.open(Overlay::Setup);
        n.open(Overlay::Dialog(DialogId(1)));
        n.open(Overlay::Dialog(DialogId(2)));
        assert_eq!(
            n.overlays(),
            &[Overlay::Setup, Overlay::Dialog(DialogId(2))]
        );
        n.open(Overlay::AddGame);
        assert_eq!(n.overlays(), &[Overlay::AddGame]);
        n.close(Overlay::AddGame);
        assert!(n.overlays().is_empty());
    }

    #[test]
    fn sidebar_navigation_resets_the_stack() {
        let mut n = Navigator::default();
        n.push(Route::Game(game()));
        n.open(Overlay::Shortcuts);
        n.go_to(Route::Statistics);
        assert_eq!(n.current(), Route::Statistics);
        assert!(!n.can_go_back());
    }
}
