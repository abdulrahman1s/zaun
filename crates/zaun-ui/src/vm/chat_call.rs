//! The call box, the watch stage and the share dialog's steps, as the page keeps them
//! between frames (`chat-calls.html` sections D–F). Owner: callui.

use zaun_core::clock::MonoInstant;

/// Which tile is shown big (the rest form the strip under it).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Tile {
    /// The user's own.
    Me,
    /// Someone else in the call, by name.
    Person(String),
    /// The game a friend in the call shares.
    Stream,
}

/// The share dialog's step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShareStep {
    /// "Share <game> with <friend>?".
    #[default]
    Ask,
    /// "Choose <game> in the next window".
    Choose,
    /// The desktop's window picker is open: "Waiting for your desktop…".
    Waiting,
    /// The picker was closed or sharing failed: the dialog closes.
    Over,
}

/// Call page state (callui).
#[derive(Clone, Debug, Default)]
pub struct CallVm {
    /// The user made the stage the tray (the stage is the default).
    pub smaller: bool,
    /// The tile shown big, if one was clicked.
    pub focus: Option<Tile>,
    /// The user asked to see everyone (a friend's shared game no longer takes the big tile).
    pub everyone: bool,
    /// The stage's height the grip set for this session.
    pub height: Option<f32>,
    /// The device menu was open in the last frame (Esc closes it first).
    pub menu_open: bool,
    /// Esc asked the device menu to close.
    pub close_menu: bool,
    /// The watched game fills the window.
    pub full: bool,
    /// The window was already full screen before (it stays so afterwards).
    pub was_full: bool,
    /// The share dialog's step.
    pub share_step: ShareStep,
    /// When the ended stream's stage appeared (it closes itself after a while).
    pub ended_at: Option<MonoInstant>,
}

impl CallVm {
    /// The first steps of `Esc`, before anything else on the page: close the device menu,
    /// then leave full screen.
    pub fn escape_over(&mut self) -> bool {
        if self.menu_open {
            self.close_menu = true;
            self.menu_open = false;
            return true;
        }
        if self.full {
            self.full = false;
            return true;
        }
        false
    }

    /// A later step of `Esc` (after the composer's): show everyone instead of a focused
    /// tile.
    pub fn unfocus(&mut self) -> bool {
        if self.focus.take().is_some() {
            self.everyone = true;
            return true;
        }
        false
    }

    /// The call ended: the next one opens as the stage, showing everyone.
    pub fn call_over(&mut self) {
        self.smaller = false;
        self.focus = None;
        self.everyone = false;
        self.menu_open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_closes_the_menu_then_full_screen_and_later_the_focused_tile() {
        let mut vm = CallVm {
            menu_open: true,
            full: true,
            focus: Some(Tile::Me),
            ..CallVm::default()
        };
        assert!(vm.escape_over());
        assert!(vm.close_menu && vm.full);
        assert!(vm.escape_over());
        assert!(!vm.full && vm.focus.is_some());
        assert!(!vm.escape_over());
        assert!(vm.unfocus());
        assert!(vm.focus.is_none());
        assert!(!vm.unfocus());
    }
}
