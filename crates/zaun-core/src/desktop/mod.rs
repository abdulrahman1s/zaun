//! Everything about being a well-behaved desktop application.
//!
//! Layer 5. See MODULES §6 and DESKTOP_INTEGRATION.
//!
//! - [`instance`]: the single-instance socket, its protocol, server and clients.
//! - [`desktop_entry`]: rendering per-game `.desktop` files.
//! - [`mime`]: the `.exe` default handler (`xdg-mime`).
//! - [`shortcuts`]: per-game menu and Desktop shortcuts.
//! - [`notify`]: desktop notifications.
//! - [`open_uri`]: opening known folders, files and compiled-in links.
//!
//! [`DesktopServices`] bundles the parts the AppService uses.

pub mod desktop_entry;
pub mod instance;
pub mod mime;
pub mod notify;
pub mod open_uri;
pub mod shortcuts;

use std::path::PathBuf;
use std::sync::Arc;

use crate::fsx::Fs;
use crate::host::CommandRunner;
use crate::ids::GameId;
use crate::paths::{AppPaths, PathEnv};

pub use mime::{MimeRegistrar, Overall as HandlerOverall, XdgDirs};
pub use open_uri::{KnownPath, Link, Opener};
pub use shortcuts::{ShortcutInfo, Shortcuts};

/// The desktop seams the AppService needs (DESKTOP-005, -006, -008).
pub struct DesktopServices {
    /// The `.exe` handler.
    pub mime: Arc<dyn MimeRegistrar>,
    /// Per-game shortcuts.
    pub shortcuts: Arc<Shortcuts>,
    /// `xdg-open`.
    pub opener: Arc<Opener>,
    /// Whether `xdg-mime` exists (otherwise the handler button explains the manual steps).
    pub mime_available: bool,
}

impl DesktopServices {
    /// The real implementations for the user's environment; `None` without a home.
    pub fn real(
        fs: Arc<dyn Fs>,
        runner: Arc<dyn CommandRunner>,
        env: &PathEnv,
        paths: &AppPaths,
    ) -> Option<Self> {
        let dirs = XdgDirs::from_env(env)?;
        let mime = mime::XdgMimeRegistrar::new(
            Arc::clone(&runner),
            Arc::clone(&fs),
            env,
            dirs.clone(),
            Some(mime_scratch(paths)),
        );
        let mime_available = mime.available();
        let desktop_dir = Shortcuts::desktop_dir_of(&*fs, env, &dirs);
        let shortcuts = Shortcuts::new(
            Arc::clone(&fs),
            dirs,
            desktop_dir,
            Some(Arc::clone(&runner)),
            env,
        );
        let opener = Opener::new(runner, &*fs, env);
        Some(Self {
            mime: Arc::new(mime),
            shortcuts: Arc::new(shortcuts),
            opener: Arc::new(opener),
            mime_available,
        })
    }
}

/// What Settings → Integration shows (published in the snapshot hub).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IntegrationState {
    /// The `.exe` default (`None` until queried).
    pub handler: Option<HandlerOverall>,
    /// `xdg-mime` is missing: show [`mime::MANUAL_STEPS`].
    pub handler_unavailable: bool,
    /// Shortcuts Zaun created.
    pub shortcuts: Vec<ShortcutInfo>,
}

impl IntegrationState {
    /// Whether `game` has a menu shortcut.
    pub fn has_shortcut(&self, game: GameId) -> bool {
        self.shortcuts.iter().any(|s| s.id == game)
    }
}

/// `<run>/mime-probe`: where the MIME sample files are written.
pub fn mime_scratch(paths: &AppPaths) -> PathBuf {
    paths.runtime_dir.join("mime-probe")
}
