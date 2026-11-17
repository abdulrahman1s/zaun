//! The only place where user-facing error text is written.
//!
//! [`describe`] is an exhaustive `match` on [`ErrorCode`]: adding a code without an entry here
//! does not compile. Titles and messages never contain paths or secrets; technical detail goes
//! to [`UserError::details`] through the caller-supplied redaction function.
//!
//! Content rules (SCREEN_SPECS_2 §16, USER_EXPERIENCE §9), enforced by the tests below: the
//! title says what happened in the user's terms, without an "Error" prefix or an exclamation
//! mark; the message has at most two sentences; neither contains a path. Where the context
//! names a file or folder, the message names it by its display name only (the last path
//! component, [`display_name`]), e.g. "“Mods”, which this game may use, no longer exists."

use super::{Error, ErrorCode, RecoveryAction, SettingsTab, UserError};
use crate::ids::{GameId, SessionId};
use crate::model::BasePreset;

/// Context for [`describe`].
pub struct DescribeCtx<'a> {
    /// The game the error is about.
    pub game: Option<GameId>,
    /// The session the error is about.
    pub session: Option<SessionId>,
    /// Redaction applied to every piece of technical detail (normally
    /// `logging::Redactor::redact_str`).
    pub redact: &'a dyn Fn(&str) -> String,
}

fn identity(s: &str) -> String {
    s.to_owned()
}

impl DescribeCtx<'static> {
    /// No game, no session, no redaction (tests and CLI output of unredacted errors).
    pub fn plain() -> Self {
        Self {
            game: None,
            session: None,
            redact: &identity,
        }
    }
}

/// The static text of one catalogue entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// What happened.
    pub title: &'static str,
    /// What it means and the likely cause.
    pub message: &'static str,
    /// Ordered recovery actions; may be empty when nothing can be done.
    pub actions: &'static [RecoveryAction],
}

const fn e(
    title: &'static str,
    message: &'static str,
    actions: &'static [RecoveryAction],
) -> Entry {
    Entry {
        title,
        message,
        actions,
    }
}

use RecoveryAction as A;

/// The catalogue entry of a code.
pub const fn entry(code: ErrorCode) -> Entry {
    use ErrorCode as C;
    match code {
        // ---- Host ----
        C::HostBwrapNotFound => e(
            "Bubblewrap is not installed",
            "EasyPlay runs every program inside a Bubblewrap sandbox and cannot start games without it.",
            &[A::OpenDoctor],
        ),
        C::HostBwrapMissingOption => e(
            "Bubblewrap is too old",
            "The installed Bubblewrap lacks an option EasyPlay needs. Updating it fixes this.",
            &[A::OpenDoctor],
        ),
        C::HostBwrapSetuidOnly => e(
            "Bubblewrap cannot create a sandbox here",
            "Bubblewrap is installed in setuid mode and this system does not allow unprivileged user namespaces.",
            &[A::OpenDoctor],
        ),
        C::HostUsernsUnavailable => e(
            "This system does not allow sandboxes",
            "Unprivileged user namespaces are turned off. The Doctor explains how to allow them on this system.",
            &[A::OpenDoctor],
        ),
        C::HostNestedUsernsUnavailable => e(
            "The compatibility runtime cannot start",
            "This system does not allow a sandbox inside a sandbox, which the Steam Linux Runtime needs.",
            &[A::OpenDoctor],
        ),
        C::HostInitMissing => e(
            "EasyPlay is incompletely installed",
            "A helper program that runs inside every sandbox is missing or cannot be run. Reinstalling EasyPlay fixes this.",
            &[A::OpenDoctor],
        ),
        C::HostUmuNotFound => e(
            "The compatibility launcher is not installed",
            "EasyPlay starts Windows programs through umu-launcher, which was not found.",
            &[A::OpenDoctor],
        ),
        C::HostUmuTooOld => e(
            "The compatibility launcher is too old",
            "The installed umu-launcher is older than the version EasyPlay supports.",
            &[A::OpenDoctor],
        ),
        C::HostUmuClosureRejected => e(
            "The compatibility launcher cannot be used safely",
            "umu-launcher is installed in a way that cannot be made visible inside the sandbox without exposing private files.",
            &[A::OpenSettings(SettingsTab::Advanced)],
        ),
        C::HostNoDisplay => e(
            "No display found",
            "EasyPlay could not find a Wayland or X11 display for games to show their windows on.",
            &[A::OpenDoctor],
        ),
        C::HostNoGpu => e(
            "No graphics device available",
            "Games will run without hardware acceleration, which is usually too slow to play.",
            &[A::OpenDoctor],
        ),
        C::HostRunningAsRoot => e(
            "EasyPlay cannot run as root",
            "Running games with administrator rights defeats the sandbox. Start EasyPlay as your normal user.",
            &[A::Quit],
        ),
        C::HostMuslUnsupported => e(
            "This system is not supported",
            "The compatibility launcher does not support musl-based systems, so nothing can be done from within EasyPlay.",
            &[],
        ),

        // ---- Sandbox ----
        C::SbxSetupFailed => e(
            "The sandbox could not be set up",
            "Bubblewrap refused to create the sandbox for this game.",
            &[A::Retry, A::ViewLog, A::CreateReport],
        ),
        C::SbxGrantMissing => e(
            "A folder this game may use is missing",
            "One of the folders or files this game was allowed to use no longer exists.",
            &[A::EditPermissions, A::Retry],
        ),
        C::SbxMountOnSymlink => e(
            "Internal planning error",
            "EasyPlay tried to place a folder on a symbolic link. This is a bug; please report it.",
            &[A::CreateReport],
        ),
        C::SbxOverlayUnsupported => e(
            "Protected game folders are not supported here",
            "This system's Bubblewrap cannot keep the game's changes separate from its folder.",
            &[A::AllowGameFolderWrites, A::EditPermissions],
        ),
        C::SbxInitNotRunnable => e(
            "The sandbox helper could not start",
            "A helper program EasyPlay places inside the sandbox could not be run.",
            &[A::OpenDoctor],
        ),
        C::SbxNoDisplayAccess => e(
            "The game would have no way to show a window",
            "The current display permissions deny every display available on this system.",
            &[A::EditPermissions],
        ),
        C::SbxRuntimeLayout => e(
            "Internal planning error",
            "EasyPlay could not lay out the compatibility runtime inside the sandbox. This is a bug; please report it.",
            &[A::CreateReport],
        ),
        C::SbxPolicyUnreadable => e(
            "This game's permissions could not be read",
            "They were saved by a newer EasyPlay or are damaged. Resetting them to Offline makes the game usable again.",
            &[A::EditPermissions],
        ),
        C::SbxDenied => e(
            "A permission is not allowed",
            "One of the folders or files would expose private or system data, so EasyPlay will not grant it.",
            &[A::EditPermissions],
        ),
        C::SbxBroadGrant => e(
            "A permission is unusually broad",
            "The game could read or change many of your files. Make sure this is what you want.",
            &[A::Dismiss],
        ),
        C::SbxGrantTargetChanged => e(
            "A permitted folder now points somewhere else",
            "A folder this game may use was replaced by a link to another place. This can be a sign of tampering.",
            &[A::EditPermissions],
        ),
        C::SbxPlanInvariant => e(
            "The launch was stopped for safety",
            "EasyPlay detected an inconsistency in the sandbox it prepared and did not start the game. This is a bug; please report it.",
            &[A::CreateReport],
        ),

        // ---- Runtime ----
        C::RtNotInstalledOffline => e(
            "The compatibility layer is not installed",
            "It needs to be downloaded, but there is no internet connection.",
            &[A::Retry],
        ),
        C::RtUnresolvable => e(
            "The compatibility layer could not be found",
            "The compatibility layer chosen for this game could not be resolved to an installed build.",
            &[A::UseDefaultRuntime, A::ChangeRuntime],
        ),
        C::RtInvalidFolder => e(
            "The compatibility layer folder is not valid",
            "The chosen folder does not contain a usable Proton build.",
            &[A::ChangeRuntime],
        ),
        C::RtUnsupportedRuntime => e(
            "This compatibility layer is not supported",
            "It needs a runtime setup that EasyPlay does not support on this system.",
            &[A::ChangeRuntime],
        ),
        C::RtVerificationFailed => e(
            "The download could not be verified",
            "The downloaded compatibility layer did not pass verification and was not installed.",
            &[A::Retry],
        ),
        C::RtNoLongerInstalled => e(
            "The selected compatibility layer is no longer installed",
            "The build this game uses was removed.",
            &[A::UseDefaultRuntime, A::ChangeRuntime],
        ),
        C::RtDownloadInterrupted => e(
            "The download was interrupted",
            "What was already downloaded is kept and the download resumes next time.",
            &[A::Retry],
        ),
        C::RtNotInstalled => e(
            "The compatibility layer is not installed",
            "EasyPlay cannot download it here. Install a Proton build with Steam or another \
             launcher, then copy the Steam Linux Runtime already on this computer from \
             Settings, Compatibility layers.",
            &[A::OpenSettings(SettingsTab::Runtimes), A::ChangeRuntime],
        ),
        C::RtSharedTampered => e(
            "The shared compatibility runtime was modified",
            "EasyPlay found changes in the shared runtime and removed it. It will be downloaded again.",
            &[A::Dismiss, A::ViewLog],
        ),
        C::RtDamaged => e(
            "Compatibility runtime files are damaged",
            "The runtime may be damaged and will be downloaded again. No game is suspected.",
            &[A::Dismiss],
        ),
        C::RtNoSpaceForCopy => e(
            "Not enough space for a private runtime copy",
            "Giving this game its own copy of the compatibility runtime needs more free disk space.",
            &[A::FreeSpaceInfo, A::OpenSettings(SettingsTab::Protection)],
        ),
        C::RtPrefixPrepareFailed => e(
            "Private Windows storage could not be prepared",
            "Setting up this game's private Windows environment failed.",
            &[A::Retry, A::ChangeRuntime, A::ViewLog],
        ),
        C::RtPrefixDamaged => e(
            "Private Windows storage looks damaged",
            "This game's private Windows environment appears to be damaged. Resetting it keeps your save data by default.",
            &[A::ResetPrefix, A::ViewLog],
        ),

        // ---- umu ----
        C::UmuInternalError => e(
            "The compatibility launcher stopped with an error",
            "umu-launcher ended unexpectedly before the game started.",
            &[A::Retry, A::ViewLog, A::CreateReport],
        ),
        C::UmuProgramDidNotStart => e(
            "The program did not start",
            "It exited immediately with an error.",
            &[A::ViewLog, A::Retry],
        ),
        C::UmuContainerFailed => e(
            "The compatibility container could not start",
            "The Steam Linux Runtime container failed to start on this system.",
            &[A::OpenDoctor, A::ViewLog],
        ),
        C::UmuFixNeedsNetwork => e(
            "A game fix needs the internet",
            "A compatibility fix for this game wants to download something, but the game has no internet access.",
            &[A::Dismiss],
        ),

        // ---- Session ----
        C::SesSupervisorNotStarted => e(
            "The game's background process did not start",
            "EasyPlay could not start the process that looks after the game while it runs.",
            &[A::Retry, A::ViewLog],
        ),
        C::SesPlanVersionMismatch => e(
            "EasyPlay was updated during the launch",
            "The launch was prepared by a different version. Trying again fixes this.",
            &[A::Retry],
        ),
        C::SesAlreadyRunning => e(
            "Already running",
            "This game is already running.",
            &[A::ShowRunningGame],
        ),
        C::SesForcedStop => e(
            "The game did not stop cleanly",
            "It did not close in time and was stopped forcefully. Unsaved progress may be lost.",
            &[A::ViewLog],
        ),
        C::SesSignalDenied => e(
            "The game could not be stopped",
            "EasyPlay was not permitted to stop the game's processes. This should not happen; a report helps fix it.",
            &[A::CreateReport],
        ),
        C::SesInterrupted => e(
            "A previous session was interrupted",
            "EasyPlay closed it using the last time it was known to be running; nothing else needs to be done.",
            &[],
        ),

        // ---- Game ----
        C::GameClosedEarly => e(
            "The game closed right after starting",
            "It may need different settings. The log can show why.",
            &[A::ViewLog],
        ),
        C::GameCrashed => e(
            "The game crashed",
            "It ended unexpectedly. Your playtime was recorded.",
            &[A::ViewLog, A::Retry],
        ),
        C::GameKilledBySystem => e(
            "The game was stopped by the system",
            "The system ended it, most likely because it ran out of memory.",
            &[A::ViewLog],
        ),

        // ---- Library and identity ----
        C::LibExeNotFound => e(
            "Program file not found",
            "The game's program file is no longer where it was. It may have been moved or deleted.",
            &[A::LocateExecutable, A::RemoveGame],
        ),
        C::LibDriveDisconnected => e(
            "The drive is not connected",
            "The drive this game is on is not connected.",
            &[A::Retry],
        ),
        C::LibExeUnreadable => e(
            "The program file cannot be read",
            "Your account is not allowed to read the game's program file.",
            &[A::Retry],
        ),
        C::LibAlreadyInLibrary => e(
            "Already in your library",
            "This program is already in the library.",
            &[A::Dismiss],
        ),
        C::LibBusy => e(
            "Another action is still running",
            "Wait for it to finish; this game is busy until then.",
            &[],
        ),
        C::LibInvalidField => e(
            "This value cannot be used",
            "Check the highlighted field; it says what is allowed.",
            &[],
        ),
        C::IdNotWindowsProgram => e(
            "This file is not a Windows program",
            "EasyPlay can only run Windows programs (.exe files and installers).",
            &[A::Dismiss],
        ),
        C::IdPeUnreadable => e(
            "Program information could not be read",
            "EasyPlay used the file name as the game's name instead; you can rename it.",
            &[],
        ),

        // ---- Files ----
        C::FsDiskFull => e(
            "The disk is full",
            "There is not enough free space to finish.",
            &[A::FreeSpaceInfo, A::Retry],
        ),
        C::FsLowSpace => e(
            "Disk space is low",
            "The game may not be able to save or update.",
            &[A::Dismiss, A::FreeSpaceInfo],
        ),
        C::FsWriteDenied => e(
            "Could not write a file",
            "Permission to write was denied.",
            &[A::Retry, A::CreateReport],
        ),
        C::FsDeleteIncomplete => e(
            "Some files could not be deleted",
            "EasyPlay will try again the next time it starts; nothing else needs to be done.",
            &[],
        ),
        C::FsReadDenied => e(
            "Cannot read the file",
            "Your account is not allowed to read it.",
            &[A::Dismiss],
        ),
        C::FsTimeout => e(
            "The location did not respond",
            "The drive or network location took too long to answer.",
            &[A::Retry],
        ),

        // ---- Database and configuration ----
        C::DbCorrupt => e(
            "The library file is damaged",
            "EasyPlay can restore a backup or continue with what could be recovered. Your games' files and saves are not affected.",
            &[A::RestoreBackup, A::OpenBackupsFolder, A::Quit],
        ),
        C::DbBusy => e(
            "The library is busy",
            "The change was not saved. Trying again usually works.",
            &[A::Retry],
        ),
        C::DbNewerVersion => e(
            "The library was created by a newer EasyPlay",
            "This version cannot open it without risking data loss. Install the newer version or restore a backup.",
            &[A::OpenBackupsFolder, A::Quit],
        ),
        C::DbSessionsRunning => e(
            "Close running games first",
            "The library needs an update, which cannot happen while games are running.",
            &[A::ShowRunningGame, A::Quit],
        ),
        C::DbBackupFailed => e(
            "Could not back up the library",
            "The update was not attempted, so nothing changed.",
            &[A::FreeSpaceInfo, A::Quit],
        ),
        C::DbMigrationFailed => e(
            "EasyPlay couldn't update your library",
            "Nothing was lost: the previous version was restored. The library is read-only until this is resolved.",
            &[A::CreateReport, A::Quit],
        ),
        C::CfgInvalidValue => e(
            "A setting was not valid",
            "It was not changed; the previous value is still in use.",
            &[],
        ),
        C::CfgUnreadable => e(
            "Settings could not be read",
            "Default settings are in use.",
            &[A::OpenSettings(SettingsTab::General)],
        ),
        C::CfgNewerVersion => e(
            "Settings are from a newer EasyPlay",
            "Default settings are in use; the settings file was not changed, so the newer version keeps working.",
            &[],
        ),

        // ---- Artwork ----
        C::ArtUnsupportedFormat => e(
            "Image format not supported",
            "Use a PNG, JPEG, WebP, ICO or BMP image.",
            &[A::Dismiss],
        ),
        C::ArtTooLarge => e(
            "The image is too large",
            "Images up to 32 MiB and 16 384 pixels on each side can be used. Choose a smaller one.",
            &[A::Dismiss],
        ),
        C::ArtUnreadable => e(
            "The image could not be read",
            "The file may be damaged. Choose another image.",
            &[A::Dismiss],
        ),
        C::ArtNotFound => e(
            "No official artwork found",
            "Steam has no artwork for this game, or EasyPlay could not tell which Steam game it is. Enter its Steam app ID or store link and try again.",
            &[A::Dismiss],
        ),
        C::ArtNoConnection => e(
            "Steam could not be reached",
            "Check the internet connection and try again.",
            &[A::Dismiss],
        ),
        C::ArtNoIcon => e(
            "This program has no icon",
            "Choose an image instead.",
            &[A::Dismiss],
        ),

        // ---- Steam games ----
        C::StmNotFound => e(
            "Steam was not found",
            "EasyPlay looked in the places Steam installs to and found no Steam library on this computer.",
            &[A::Dismiss],
        ),
        C::StmGameGone => e(
            "Steam no longer lists this game",
            "It may have been uninstalled in Steam, or the drive it is on is not connected. What you set for it here is kept.",
            &[A::Dismiss],
        ),
        C::StmClientUnavailable => e(
            "Steam could not be asked",
            "EasyPlay could not start Steam to pass the request on. Open Steam and try again.",
            &[A::CopyDetails],
        ),
        C::StmGameRunning => e(
            "The game is running",
            "Programs can run in the game's Windows storage only while the game is closed. Close the game in Steam first.",
            &[A::Dismiss],
        ),
        C::StmNoPrefix => e(
            "This game has no Windows storage yet",
            "Steam creates it the first time the game is played through Proton. Play it once in Steam, then try again.",
            &[A::Dismiss],
        ),
        C::StmLayerUnavailable => e(
            "The compatibility layer Steam uses cannot be used",
            "The Proton build that last ran this game is missing or cannot run here. Play the game once in Steam, then try again.",
            &[A::Dismiss],
        ),
        C::StmNotForSteam => e(
            "Steam handles this",
            "This game is run by Steam, so this is changed in Steam. Open the game in Steam to change it.",
            &[A::Dismiss],
        ),
        C::StmSteamOpen => e(
            "Steam is open",
            "Steam rewrites its settings when it closes, so launch options can be changed only while Steam is closed. Close Steam, then save again.",
            &[A::Dismiss],
        ),
        C::StmSettingsUnwritable => e(
            "Steam's settings could not be changed",
            "EasyPlay could not find or safely change the settings file of your Steam account. Nothing was changed; set the launch options in Steam instead.",
            &[A::CopyDetails],
        ),

        // ---- Chat ----
        C::ChtUnavailable => e(
            "Chat is not included in this build",
            "This copy of EasyPlay was built without the chat engine. Everything else works as usual.",
            &[A::Dismiss],
        ),
        C::ChtEngineFailing => e(
            "Chat stopped",
            "The chat engine stopped several times in a row; your friends and messages are kept. Turn Chat off and on again to retry.",
            &[A::CopyDetails],
        ),
        C::ChtDatabase => e(
            "Your messages could not be opened",
            "The encrypted chat database could not be read; it may be damaged, or its key file was changed. Nothing was deleted.",
            &[A::CopyDetails],
        ),
        C::ChtNoConnection => e(
            "The relays cannot be reached",
            "Messages wait on your computer and are sent when a relay can be reached again. Check the internet connection.",
            &[A::Dismiss],
        ),
        C::ChtBadLink => e(
            "This is not a friend's link",
            "Paste the whole link your friend shared, exactly as they sent it.",
            &[A::Dismiss],
        ),
        C::ChtRequestFailed => e(
            "That did not work",
            "The chat engine refused the request. The details say why.",
            &[A::CopyDetails],
        ),

        // ---- Integration and internal ----
        C::DskDefaultHandlerFailed => e(
            "Could not change the default application",
            "The details show how to change it by hand.",
            &[A::CopyDetails],
        ),
        C::DskShortcutFailed => e(
            "Could not create the shortcut",
            "Writing the shortcut file failed.",
            &[A::Retry],
        ),
        C::DskInstanceUnresponsive => e(
            "EasyPlay is not responding",
            "Another EasyPlay window did not answer.",
            &[A::Retry],
        ),
        C::IntTaskFailed => e(
            "A background task failed",
            "An internal task stopped unexpectedly. Trying again usually works; a report helps fix it.",
            &[A::Retry, A::CreateReport],
        ),
        C::IntFatal => e(
            "EasyPlay has to close",
            "An unrecoverable internal error happened. Running games are not affected.",
            &[A::CreateReport],
        ),
        C::IntNotAvailable => e(
            "Not available yet",
            "This version of EasyPlay does not include this feature.",
            &[A::Dismiss],
        ),
    }
}

/// Recovery actions whose argument depends on the error (e.g. which preset to switch to).
fn adjust_actions(code: ErrorCode, base: &[RecoveryAction], err: &Error) -> Vec<RecoveryAction> {
    let mut actions = base.to_vec();
    if code == ErrorCode::GameClosedEarly && err.context_value("suggest_preset") == Some("online") {
        actions.push(RecoveryAction::SwitchPreset(BasePreset::Online));
    }
    actions
}

/// The name a user knows a file or folder by: its last path component, without control
/// characters, at most 60 characters. `None` for an empty or root path.
pub fn display_name(path: &str) -> Option<String> {
    let last = path.trim_end_matches('/').rsplit('/').next()?;
    let clean: String = last
        .chars()
        .map(|c| if c.is_control() { '·' } else { c })
        .take(60)
        .collect();
    (!clean.trim().is_empty() && clean != "~").then_some(clean)
}

/// A message naming the file or folder of the context, for codes about one place.
fn interpolated(code: ErrorCode, err: &Error) -> Option<String> {
    use ErrorCode as C;
    let name = || {
        ["path", "folder", "file"]
            .iter()
            .find_map(|k| err.context_value(k))
            .and_then(display_name)
    };
    Some(match code {
        C::SbxGrantMissing => format!(
            "\u{201c}{}\u{201d}, which this game may use, no longer exists. It may have been moved or its drive is not connected.",
            name()?
        ),
        C::SbxDenied => format!(
            "Sharing \u{201c}{}\u{201d} would expose private or system data, so EasyPlay will not grant it.",
            name()?
        ),
        C::LibExeNotFound => format!(
            "\u{201c}{}\u{201d} is no longer where it was. It may have been moved or deleted.",
            name()?
        ),
        C::LibExeUnreadable => format!(
            "Your account is not allowed to read \u{201c}{}\u{201d}.",
            name()?
        ),
        C::LibDriveDisconnected => format!(
            "The drive \u{201c}{}\u{201d} this game is on is not connected.",
            err.context_value("drive")
                .and_then(display_name)
                .or_else(name)?
        ),
        C::FsWriteDenied => format!(
            "Permission to change \u{201c}{}\u{201d} was denied.",
            name()?
        ),
        C::FsReadDenied => format!(
            "Your account is not allowed to read \u{201c}{}\u{201d}.",
            name()?
        ),
        C::FsDiskFull => format!(
            "There is not enough free space on the disk that holds \u{201c}{}\u{201d}.",
            name()?
        ),
        _ => return None,
    })
}

/// Build the technical detail text: code, context and source chain, all redacted.
fn details(err: &Error, ctx: &DescribeCtx<'_>) -> String {
    let mut out = String::from(err.code.as_str());
    for (k, v) in &err.context {
        out.push('\n');
        out.push_str(k);
        out.push_str(": ");
        out.push_str(&(ctx.redact)(v));
    }
    let chain = err.chain_text();
    if !chain.is_empty() {
        out.push_str("\ncause: ");
        out.push_str(&(ctx.redact)(&chain));
    }
    out
}

/// Describe an error for the user.
pub fn describe(err: &Error, ctx: &DescribeCtx<'_>) -> UserError {
    let entry = entry(err.code);
    UserError {
        code: err.code,
        severity: err.code.severity(),
        title: entry.title.to_owned(),
        message: interpolated(err.code, err).unwrap_or_else(|| entry.message.to_owned()),
        actions: adjust_actions(err.code, entry.actions, err),
        details: Some(details(err, ctx)),
        game: ctx.game,
        session: ctx.session,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_describes_without_panic() {
        for code in ErrorCode::ALL {
            let u = describe(&Error::new(*code), &DescribeCtx::plain());
            assert!(!u.title.trim().is_empty(), "{code}");
            assert!(!u.message.trim().is_empty(), "{code}");
            // No placeholders left unfilled, no paths in user text.
            for text in [&u.title, &u.message] {
                assert!(!text.contains('{') && !text.contains('}'), "{code}: {text}");
                assert!(!text.contains("TODO"), "{code}");
                assert!(!text.contains('/'), "{code}: {text}");
                assert!(!text.contains('~'), "{code}: {text}");
            }
            assert_eq!(u.severity, code.severity());
        }
    }

    #[test]
    fn actions_or_explicit_none() {
        // Entries without actions must say why nothing can be done or be informational.
        for code in ErrorCode::ALL {
            let e = entry(*code);
            if e.actions.is_empty() {
                assert!(!e.message.is_empty(), "{code}");
            }
        }
        assert_eq!(
            entry(ErrorCode::LibExeNotFound).actions,
            &[RecoveryAction::LocateExecutable, RecoveryAction::RemoveGame]
        );
    }

    #[test]
    fn details_are_redacted() {
        let redact = |s: &str| s.replace("/home/sam", "~");
        let ctx = DescribeCtx {
            game: None,
            session: None,
            redact: &redact,
        };
        let err = Error::new(ErrorCode::FsWriteDenied)
            .with("path", "/home/sam/Games/save.dat")
            .with_source(std::io::Error::other("denied at /home/sam/Games"));
        let u = describe(&err, &ctx);
        let d = u.details.unwrap();
        assert!(d.contains("path: ~/Games/save.dat"), "{d}");
        assert!(d.contains("cause: denied at ~/Games"), "{d}");
        assert!(!d.contains("/home/sam"));
    }

    fn sentences(text: &str) -> usize {
        text.split_terminator(['.', '?'])
            .filter(|s| !s.trim().is_empty())
            .count()
    }

    #[test]
    fn copy_follows_the_content_rules() {
        for code in ErrorCode::ALL {
            let e = entry(*code);
            let t = e.title.to_lowercase();
            assert!(
                !t.starts_with("error") && !t.starts_with("failed") && !t.contains("error:"),
                "{code}: title starts with an error word: {}",
                e.title
            );
            for text in [e.title, e.message] {
                assert!(!text.contains('!'), "{code}: exclamation mark in {text}");
                assert!(!text.contains("EP-"), "{code}: code in user text");
            }
            assert!(!e.title.ends_with('.'), "{code}: title is a heading");
            assert!(e.title.len() <= 60, "{code}: title too long");
            assert!(
                sentences(e.message) <= 2,
                "{code}: more than two sentences: {}",
                e.message
            );
        }
    }

    /// Codes that offer no action say why nothing is needed or possible (ERROR_HANDLING §1.1).
    #[test]
    fn every_entry_has_a_way_forward() {
        use ErrorCode as C;
        let explained = [
            C::HostMuslUnsupported,
            C::LibBusy,
            C::LibInvalidField,
            C::IdPeUnreadable,
            C::FsDeleteIncomplete,
            C::SesInterrupted,
            C::CfgInvalidValue,
            C::CfgNewerVersion,
        ];
        for code in ErrorCode::ALL {
            let e = entry(*code);
            assert_eq!(
                e.actions.is_empty(),
                explained.contains(code),
                "{code}: actions {:?}",
                e.actions
            );
        }
        for code in explained {
            let m = entry(code).message.to_lowercase();
            assert!(
                [
                    "nothing",
                    "wait",
                    "check",
                    "rename",
                    "previous value",
                    "keeps working"
                ]
                .iter()
                .any(|w| m.contains(w)),
                "{code}: {m}"
            );
        }
    }

    #[test]
    fn places_are_named_by_display_name_only() {
        let err = Error::new(ErrorCode::SbxGrantMissing).with("path", "/home/sam/Games/Mods");
        let u = describe(&err, &DescribeCtx::plain());
        assert!(u.message.contains("\u{201c}Mods\u{201d}"), "{}", u.message);
        assert!(!u.message.contains('/'));
        let err = Error::new(ErrorCode::LibExeNotFound).with("path", "/mnt/g/a\nb.exe");
        let u = describe(&err, &DescribeCtx::plain());
        assert!(u.message.contains("a·b.exe"), "{}", u.message);
        // Without a path the catalogue text is used.
        let u = describe(&Error::new(ErrorCode::FsWriteDenied), &DescribeCtx::plain());
        assert_eq!(u.message, entry(ErrorCode::FsWriteDenied).message);
        assert_eq!(display_name("/"), None);
        assert_eq!(display_name("~"), None);
        assert_eq!(display_name("/a/b/"), Some("b".into()));
        for code in ErrorCode::ALL {
            let err = Error::new(*code)
                .with("path", "/home/sam/x/y")
                .with("drive", "/run/media/sam/Games");
            let u = describe(&err, &DescribeCtx::plain());
            assert!(!u.message.contains('/'), "{code}: {}", u.message);
            assert!(sentences(&u.message) <= 2, "{code}: {}", u.message);
        }
    }

    #[test]
    fn preset_suggestion() {
        let err = Error::new(ErrorCode::GameClosedEarly).with("suggest_preset", "online");
        let u = describe(&err, &DescribeCtx::plain());
        assert_eq!(
            u.actions.last(),
            Some(&RecoveryAction::SwitchPreset(BasePreset::Online))
        );
    }
}
