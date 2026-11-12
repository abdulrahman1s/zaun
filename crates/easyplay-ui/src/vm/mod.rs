//! View-models: screen state that is not domain state (selection, text being typed, expanded
//! sections, pending queries, validation messages). Never references `egui::Ui`. See MODULES §8.

pub mod debug;
pub mod details;
pub mod dialogs;
pub mod launch;
pub mod library;
pub mod screens;
pub mod setup;
pub mod toasts;

use easyplay_core::clock::MonoInstant;

/// A value that arrives asynchronously (ARCHITECTURE §5.1). The error type is generic until
/// `UserError` is wired in with SVC-006.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AsyncValue<T, E = String> {
    /// Not requested.
    #[default]
    Idle,
    /// Requested at `since` (skeletons appear after 150 ms).
    Loading {
        /// When the request was sent.
        since: MonoInstant,
    },
    /// Arrived.
    Ready(T),
    /// Failed.
    Failed(E),
}

impl<T, E> AsyncValue<T, E> {
    /// Whether a request is outstanding.
    pub fn is_loading(&self) -> bool {
        matches!(self, AsyncValue::Loading { .. })
    }

    /// The value, if ready.
    pub fn ready(&self) -> Option<&T> {
        match self {
            AsyncValue::Ready(v) => Some(v),
            _ => None,
        }
    }

    /// Whether a skeleton should show at `now` (loading for longer than the skeleton delay).
    pub fn show_skeleton(&self, now: MonoInstant) -> bool {
        match self {
            AsyncValue::Loading { since } => {
                now.saturating_duration_since(*since).as_secs_f32()
                    >= crate::theme::tokens::timing::SKELETON_DELAY
            }
            _ => false,
        }
    }
}
