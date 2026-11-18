//! Asking Steam to do things, and seeing what it runs (ADR-0031 §5, §7).
//!
//! Zaun never starts a Steam game itself: it hands a `steam://` request to the Steam
//! client of the installation that lists the game, as a detached program Zaun does not
//! own or wait for ([`Handoff`], implemented by `session::handoff`). The requests used:
//!
//! | Request | URL | What Steam does |
//! |---|---|---|
//! | Play | `steam://rungameid/<appid>` | starts the game as if Play were pressed in Steam (starts Steam first when it is not running) |
//! | Update in Steam | `steam://open/downloads` | shows Steam's downloads, where waiting updates are started or resumed |
//! | Open in Steam | `steam://nav/games/details/<appid>` | shows the game in Steam's library (its Properties hold the Proton choice and launch options) |
//! | Close Steam | `-shutdown` (the client's option, not a URL) | the running client closes itself, so its settings file can be changed (ADR-0031 §6a) |
//!
//! No `steam://` request updates one installed game on demand (TECHNICAL_RESEARCH §8, ADR-0031
//! §7), so "Update in Steam" opens the downloads page.
//!
//! Running games are recognised by the process Steam starts for every launch,
//! `…/ubuntu12_32/reaper SteamLaunch AppId=<appid> -- …`, read from `/proc/<pid>/cmdline`
//! (never `environ`). Zaun does not own that process tree, so it offers no Stop.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::fsx::Fs;
use crate::host::CommandSpec;
use crate::model::SteamInstallKind;
use crate::procfs::{ProcIdentity, ProcSource};

pub use crate::model::SteamRequest;

/// Starts a program Zaun does not own: detached (its own session, no shared descriptors,
/// standard streams on `/dev/null`), never waited for. Implemented by `session::handoff`;
/// tests record the commands.
pub trait Handoff: Send + Sync {
    /// Start `cmd` detached. `cmd.timeout` is ignored.
    fn start_detached(&self, cmd: &CommandSpec) -> Result<(), Error>;
}

/// Variables never passed to Steam: Zaun's own.
fn passed(name: &str) -> bool {
    !name.starts_with("ZAUN_")
}

/// The command that hands `request` to the Steam client of `kind`. `env` is the launcher's
/// environment (passed on, except Zaun's own variables, as a desktop launcher would);
/// `root` is the installation's root (native fallback when `steam` is not on `PATH`).
///
/// - native: `steam <url>` (from `PATH`), else `<root>/steam.sh <url>`;
/// - Flatpak: `flatpak run com.valvesoftware.Steam <url>`;
/// - Snap: `snap run steam <url>`.
///
/// `None` when the program to run is not installed.
pub fn command(
    fs: &dyn Fs,
    kind: SteamInstallKind,
    root: &Path,
    env: &BTreeMap<String, String>,
    request: SteamRequest,
) -> Option<CommandSpec> {
    let dirs =
        crate::host::exec::path_dirs(env.get("PATH").map(|s| std::ffi::OsStr::new(s.as_str())));
    let find = |name: &str| crate::host::exec::find_in(fs, &dirs, name);
    let url = request.argument();
    let (program, mut args): (PathBuf, Vec<OsString>) = match kind {
        SteamInstallKind::Native => {
            let program = find("steam").or_else(|| {
                let sh = root.join("steam.sh");
                fs.metadata_follow(&sh)
                    .is_ok_and(|m| m.is_file())
                    .then_some(sh)
            })?;
            (program, Vec::new())
        }
        SteamInstallKind::Flatpak => (
            find("flatpak")?,
            vec!["run".into(), super::roots::FLATPAK_APP_ID.into()],
        ),
        SteamInstallKind::Snap => (find("snap")?, vec!["run".into(), "steam".into()]),
    };
    args.push(url.into());
    let mut spec = CommandSpec::new(&program, &args);
    spec.env = env
        .iter()
        .filter(|(k, _)| passed(k))
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect();
    Some(spec)
}

fn arg_str(a: &OsString) -> &str {
    a.to_str().unwrap_or("")
}

/// The app ID in a `reaper SteamLaunch AppId=<n> -- …` command line.
pub fn launched_app_id(argv: &[OsString]) -> Option<u32> {
    let program = argv.first().map(arg_str)?;
    if !program.ends_with("/reaper") && program != "reaper" {
        return None;
    }
    let before_cmd = argv.iter().skip(1).take_while(|a| arg_str(a) != "--");
    let mut steam_launch = false;
    let mut app = None;
    for a in before_cmd {
        let a = arg_str(a);
        if a == "SteamLaunch" {
            steam_launch = true;
        } else if let Some(v) = a.strip_prefix("AppId=") {
            app = v.parse::<u32>().ok().filter(|v| *v > 0);
        }
    }
    app.filter(|_| steam_launch)
}

/// Steam games running now: app ID → the `reaper` process Steam started for it. Only
/// processes named `reaper` are inspected.
pub fn running_games(proc: &dyn ProcSource) -> BTreeMap<u32, ProcIdentity> {
    let mut out = BTreeMap::new();
    let Ok(entries) = proc.snapshot() else {
        return out;
    };
    for e in entries
        .iter()
        .filter(|e| e.comm == "reaper" && !e.state.is_gone())
    {
        if let Ok(Some(argv)) = proc.cmdline(e.pid)
            && let Some(app) = launched_app_id(&argv)
        {
            out.entry(app).or_insert_with(|| e.identity());
        }
    }
    out
}

/// Process names (`comm`, at most 15 bytes) of a running Steam client: the client itself
/// (`ubuntu12_32/steam`, also the `steam` launcher), its start script and its web helper,
/// which lives as long as the client's window does.
pub const CLIENT_COMMS: [&str; 3] = ["steam", "steam.sh", "steamwebhelper"];

/// Whether a Steam client runs now (any installation: Flatpak and Snap processes are seen
/// from the host too). Steam rewrites `localconfig.vdf` when it exits, so Zaun changes
/// that file only while this is false. When `/proc` cannot be read, Steam is taken to be
/// running: a wrong "running" only delays a change, a wrong "closed" loses it.
pub fn client_running(proc: &dyn ProcSource) -> bool {
    let Ok(entries) = proc.snapshot() else {
        return true;
    };
    entries
        .iter()
        .any(|e| !e.state.is_gone() && CLIENT_COMMS.contains(&e.comm.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &[&str]) -> Vec<OsString> {
        s.iter().map(OsString::from).collect()
    }

    #[test]
    fn urls() {
        assert_eq!(SteamRequest::Run(440).url(), "steam://rungameid/440");
        assert_eq!(SteamRequest::Downloads.url(), "steam://open/downloads");
        assert_eq!(
            SteamRequest::Details(440).url(),
            "steam://nav/games/details/440"
        );
        assert_eq!(SteamRequest::Shutdown.argument(), "-shutdown");
        assert_eq!(SteamRequest::Run(440).argument(), "steam://rungameid/440");
    }

    #[test]
    fn reaper_command_lines() {
        let real = argv(&[
            "/home/p/.local/share/Steam/ubuntu12_32/reaper",
            "SteamLaunch",
            "AppId=1174180",
            "--",
            "/home/p/.local/share/Steam/ubuntu12_32/steam-launch-wrapper",
            "--",
            "AppId=9",
        ]);
        assert_eq!(launched_app_id(&real), Some(1_174_180));
        // `AppId=` after `--` belongs to the game's command line.
        assert_eq!(
            launched_app_id(&argv(&["reaper", "SteamLaunch", "--", "AppId=5"])),
            None
        );
        assert_eq!(launched_app_id(&argv(&["reaper", "AppId=5"])), None);
        assert_eq!(
            launched_app_id(&argv(&["/usr/bin/other", "SteamLaunch", "AppId=5"])),
            None
        );
        assert_eq!(launched_app_id(&[]), None);
    }
}
