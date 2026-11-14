//! Onboarding (SCREEN_SPECS §1, ADR-0018 opt-in).

use crate::bridge::{CheckState, HandlerState, HostCheck};
use crate::vm::screens::OnboardingStep;

/// Banner after skipping a failing check.
pub const BANNER: &str = "Games can't start until a system check is fixed.";
/// Button.
pub const CHECK_AGAIN: &str = "Check again";
/// Button.
pub const CONTINUE: &str = "Continue";
/// Button.
pub const FINISH: &str = "Start using EasyPlay";
/// Button.
pub const BACK: &str = "Back";
/// Button.
pub const SKIP: &str = "Skip for now";
/// Disabled reason.
pub const BLOCKED: &str = "Fix the failed check to continue";
/// Slow probe.
pub const STILL_CHECKING: &str = "Still checking…";
/// Fix.
pub const TO_FIX: &str = "To turn it on, run:";
/// Button.
pub const COPY: &str = "Copy";
/// Button accessible name.
pub const COPY_COMMAND: &str = "Copy command";
/// Button: adopt umu's runtime (D-15).
pub const USE_UMU_RUNTIME: &str = "Use umu's runtime";
/// Adopting.
pub const ADOPTING: &str = "Copying umu's runtime…";
/// Layer step.
pub const NO_LAYER: &str = "No compatibility layer is installed yet. EasyPlay can download one now (about 1 GB), or when you first start a game.";
/// Button.
pub const DOWNLOAD: &str = "Download now";
/// Button.
pub const LATER: &str = "Later";
/// Handler step.
pub const MAKE_DEFAULT: &str = "Make EasyPlay the default";
/// Handler step.
pub const IS_DEFAULT: &str = "EasyPlay opens Windows programs";
/// Artwork opt-in.
pub const ARTWORK_TITLE: &str = "Official artwork";
/// Artwork opt-in (names the server and the data sent, ADR-0018).
pub const ARTWORK_OPT_IN: &str =
    "Download covers from Steam's image server (shared.steamstatic.com)";
/// Artwork opt-in detail.
pub const ARTWORK_DETAIL: &str = "Only a game's Steam app ID is sent, and only for games that have one. You can change this later in Settings.";
/// Welcome points.
pub const WELCOME_POINTS: [&str; 3] = [
    "Double-click a Windows game in your file manager to play it.",
    "Each game runs in its own protected space with only the access you choose.",
    "Your playtime and library stay on this computer.",
];

/// Step titles.
pub fn step_title(s: OnboardingStep) -> &'static str {
    match s {
        OnboardingStep::Welcome => "Welcome to EasyPlay",
        OnboardingStep::SystemCheck => "Checking your system",
        OnboardingStep::Layer => "Compatibility layer",
        OnboardingStep::Handler => "Opening Windows programs",
    }
}

/// Step bodies.
pub fn step_body(s: OnboardingStep) -> &'static str {
    match s {
        OnboardingStep::Welcome => "Play Windows games on Linux in a few clicks.",
        OnboardingStep::SystemCheck => "EasyPlay needs a few things to run Windows games.",
        OnboardingStep::Layer => "Windows games run through a compatibility layer.",
        OnboardingStep::Handler => "EasyPlay can open .exe files when you double-click them.",
    }
}

/// "Step 2 of 4: Checking your system".
pub fn step_announcement(i: usize, n: usize, title: &str) -> String {
    format!("Step {i} of {n}: {title}")
}

/// "Private namespaces: turned off; fix available".
pub fn check_name(c: &HostCheck) -> String {
    let state = match c.state {
        CheckState::Pass => "passed",
        CheckState::Warn => "works with a caveat",
        CheckState::Fail => "failed",
        CheckState::Pending => "checking",
    };
    let fix = if c.fix_command.is_some() {
        "; fix available"
    } else {
        ""
    };
    format!("{}: {}, {state}{fix}", c.name, c.result.to_lowercase())
}

/// "Found UMU-Proton 10.0-4".
pub fn found_layer(label: &str) -> String {
    format!("Found {label}")
}

/// "Downloading UMU-Proton…".
pub fn downloading(label: &str) -> String {
    format!("Downloading {label}…")
}

/// "Right now .exe files open with Wine".
pub fn handler_now(h: &HandlerState) -> String {
    match h {
        HandlerState::EasyPlay => "Right now .exe files open with EasyPlay.".to_owned(),
        HandlerState::Other(n) => format!("Right now .exe files open with {n}."),
        HandlerState::None => "Right now nothing opens .exe files.".to_owned(),
        HandlerState::Unknown => String::new(),
    }
}
