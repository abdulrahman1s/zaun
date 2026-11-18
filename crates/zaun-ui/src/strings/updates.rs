//! Updates in Settings › About and the toast that announces a version (ADR-0040). The help
//! text names the server and what it learns before the switch can be turned on (PRIVACY §1).

use zaun_core::model::InstallChannel;

texts! {
    /// The group's heading.
    GROUP = "updates.group";
    /// The switch.
    CHECK = "updates.check";
    /// Under the switch: the server and the data sent.
    CHECK_HELP = "updates.check_help";
    /// The version row's button.
    CHECK_NOW = "updates.check_now";
    /// The version row's help while a check runs.
    CHECKING = "updates.checking";
    /// … before the first check.
    NEVER_CHECKED = "updates.never_checked";
    /// … while checks are off.
    CHECKS_OFF = "updates.checks_off";
    /// Under the heading of a build without an update key.
    NOT_CONFIGURED = "updates.not_configured";
    /// The release notes button.
    NOTES = "updates.notes";
    /// The download button of an owned install.
    DOWNLOAD = "updates.download";
    /// The download button while it runs.
    DOWNLOADING = "updates.downloading";
    /// Under the available version of an owned install.
    DOWNLOAD_HELP = "updates.download_help";
    /// The button that installs a downloaded version.
    RESTART = "updates.restart";
    /// Under the downloaded version.
    READY_HELP = "updates.ready_help";
    /// … while a game runs.
    READY_WAITS = "updates.ready_waits";
}

/// "Zaun 0.2.0 is available" (also the toast).
pub fn available(version: &str) -> String {
    rust_i18n::t!("updates.available", v = version).into_owned()
}

/// "Zaun 0.2.0 is ready to install".
pub fn ready(version: &str) -> String {
    rust_i18n::t!("updates.ready", v = version).into_owned()
}

/// "Up to date · checked today".
pub fn up_to_date(when: &str) -> String {
    rust_i18n::t!("updates.up_to_date", when = when).into_owned()
}

/// "Checked today" (a newer version is shown below it).
pub fn checked(when: &str) -> String {
    rust_i18n::t!("updates.checked", when = when).into_owned()
}

/// "Couldn't check: The update server could not be reached …".
pub fn failed(why: &str) -> String {
    rust_i18n::t!("updates.failed", why = why).into_owned()
}

/// "Zaun 0.2.0 did not start, so the version you had before was put back."
pub fn rolled_back(version: &str) -> String {
    rust_i18n::t!("updates.rolled_back", v = version).into_owned()
}

/// How a copy installed this way is updated (shown under an available version).
pub fn how_to_update(c: InstallChannel) -> &'static str {
    crate::i18n::text(match c {
        InstallChannel::Nix => "updates.how.nix",
        InstallChannel::Flatpak => "updates.how.flatpak",
        InstallChannel::Package => "updates.how.package",
        InstallChannel::Tarball { owned: true } => "updates.how.self",
        InstallChannel::Tarball { owned: false } => "updates.how.tarball_system",
        InstallChannel::Other => "updates.how.elsewhere",
    })
}
