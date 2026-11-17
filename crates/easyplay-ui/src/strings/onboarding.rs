//! Onboarding (SCREEN_SPECS §1, ADR-0018 opt-in).

use crate::bridge::{CheckState, HandlerState, HostCheck};
use crate::vm::screens::OnboardingStep;

texts! {
    /// Banner after skipping a failing check.
    BANNER = "onboarding.banner";
    /// Button.
    CHECK_AGAIN = "onboarding.check_again";
    /// Button.
    CONTINUE = "onboarding.continue";
    /// Button.
    FINISH = "onboarding.finish";
    /// Button.
    BACK = "onboarding.back";
    /// Button.
    SKIP = "onboarding.skip";
    /// Tooltip of "Skip for now", and why Continue is off.
    SKIP_TIP = "onboarding.skip_tip";
    /// Slow probe.
    STILL_CHECKING = "onboarding.still_checking";
    /// Fix.
    TO_FIX = "onboarding.to_fix";
    /// Button.
    COPY = "onboarding.copy";
    /// Button accessible name.
    COPY_COMMAND = "onboarding.copy_command";
    /// Button: adopt umu's runtime (D-15).
    USE_UMU_RUNTIME = "onboarding.use_umu_runtime";
    /// Adopting.
    ADOPTING = "onboarding.adopting";
    /// Layer step: nothing installed yet.
    NO_LAYER_TITLE = "onboarding.no_layer_title";
    /// Layer step.
    NO_LAYER = "onboarding.no_layer";
    /// Layer step: one is installed.
    LAYER_READY = "onboarding.layer_ready";
    /// Layer step: the download, in bytes so far.
    DOWNLOADED = "onboarding.downloaded";
    /// Button.
    DOWNLOAD = "onboarding.download";
    /// Button.
    LATER = "onboarding.later";
    /// Handler step: the row.
    EXE_FILES = "onboarding.exe_files";
    /// Handler step.
    MAKE_DEFAULT = "onboarding.make_default";
    /// Handler step, once EasyPlay opens them.
    IS_DEFAULT = "onboarding.is_default";
    /// Handler step: EasyPlay is the default (the glyph's accessible name).
    DEFAULT_DONE = "onboarding.default_done";
    /// Artwork opt-in.
    ARTWORK_TITLE = "onboarding.artwork_title";
    /// Artwork opt-in (names the server and the data sent, ADR-0018).
    ARTWORK_HELP = "onboarding.artwork_help";
}
/// Welcome points.
#[allow(non_snake_case)]
pub fn WELCOME_POINTS() -> [&'static str; 3] {
    [
        crate::i18n::text("onboarding.welcome_points.double_click_a_windows"),
        crate::i18n::text("onboarding.welcome_points.each_game_runs_in"),
        crate::i18n::text("onboarding.welcome_points.your_playtime_and_library"),
    ]
}

/// Step titles.
pub fn step_title(s: OnboardingStep) -> &'static str {
    match s {
        OnboardingStep::Welcome => crate::i18n::text("onboarding.step_title.welcome_to_easyplay"),
        OnboardingStep::SystemCheck => {
            crate::i18n::text("onboarding.step_title.checking_your_system")
        }
        OnboardingStep::Layer => crate::i18n::text("onboarding.step_title.compatibility_layer"),
        OnboardingStep::Handler => {
            crate::i18n::text("onboarding.step_title.opening_windows_programs")
        }
    }
}

/// Step bodies.
pub fn step_body(s: OnboardingStep) -> &'static str {
    match s {
        OnboardingStep::Welcome => crate::i18n::text("onboarding.step_body.play_windows_games_on"),
        OnboardingStep::SystemCheck => {
            crate::i18n::text("onboarding.step_body.easyplay_needs_a_few")
        }
        OnboardingStep::Layer => {
            crate::i18n::text("onboarding.step_body.windows_games_run_through")
        }
        OnboardingStep::Handler => crate::i18n::text("onboarding.step_body.easyplay_can_open_exe"),
    }
}

/// "2 of 4", at the foot of a step.
pub fn step_of(i: usize, n: usize) -> String {
    rust_i18n::t!("onboarding.step_of", i = i, n = n).into_owned()
}

/// "Step 2 of 4: Checking your system".
pub fn step_announcement(i: usize, n: usize, title: &str) -> String {
    rust_i18n::t!("onboarding.step_announcement", i = i, n = n, title = title).into_owned()
}

/// "Private namespaces: turned off; fix available".
pub fn check_name(c: &HostCheck) -> String {
    let key = match (c.state, c.fix_command.is_some()) {
        (CheckState::Pass, false) => "onboarding.check_name.pass",
        (CheckState::Pass, true) => "onboarding.check_name.pass_fix",
        (CheckState::Warn, false) => "onboarding.check_name.warn",
        (CheckState::Warn, true) => "onboarding.check_name.warn_fix",
        (CheckState::Fail, false) => "onboarding.check_name.fail",
        (CheckState::Fail, true) => "onboarding.check_name.fail_fix",
        (CheckState::Pending, false) => "onboarding.check_name.pending",
        (CheckState::Pending, true) => "onboarding.check_name.pending_fix",
    };
    rust_i18n::t!(key, name = c.name, result = c.result.to_lowercase()).into_owned()
}

/// "Found UMU-Proton 10.0-4".
pub fn found_layer(label: &str) -> String {
    rust_i18n::t!("onboarding.found_layer", label = label).into_owned()
}

/// "Downloading UMU-Proton…".
pub fn downloading(label: &str) -> String {
    rust_i18n::t!("onboarding.downloading", label = label).into_owned()
}

/// The help under ".exe files": who opens them now.
pub fn handler_now(h: &HandlerState) -> String {
    match h {
        HandlerState::EasyPlay => IS_DEFAULT().to_owned(),
        HandlerState::Other(n) => rust_i18n::t!("onboarding.handler_other", name = n).into_owned(),
        HandlerState::None => crate::i18n::text("onboarding.handler_none").to_owned(),
        HandlerState::Unknown => String::new(),
    }
}
