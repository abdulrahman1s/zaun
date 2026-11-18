//! Troubleshooting evidence (USER_FLOWS UF-9, ERR-006): what is known about a launch that
//! failed or a game that closed right after starting, and the suggestions it leads to, most
//! likely first.
//!
//! | Evidence | Suggestions |
//! |---|---|
//! | Known umu / container / sandbox error | The catalogue's recovery actions |
//! | Offline + online-capable | *Switch to Online* |
//! | Game folder read-only or "this file only", and access-denied writes in the log | *Allow changes in the game folder*, *Choose a game folder* |
//! | 32-bit program, and the log shows a missing 32-bit graphics driver | *Open the Doctor* (D-12 names the package) |
//! | Nothing specific | *Try a different compatibility layer*, *Try Standard protection*, *Reset private Windows storage* |
//!
//! *Try again*, *View log* and *Copy details* are always offered by the problem panel and are
//! not part of the list. Suggestions already tried for this failure are removed by the UI
//! (`LaunchVm::tried`), so the list shrinks instead of repeating.

use crate::error::{ErrorCode, RecoveryAction};
use crate::model::{BasePreset, GameDirAccess, NetworkAccess};

/// The bytes of `game.log` scanned (its tail).
pub const LOG_TAIL_BYTES: usize = 64 * 1024;

/// What the log tail shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LogFindings {
    /// A write was refused (permission denied, read-only file system).
    pub access_denied_writes: bool,
    /// A 32-bit graphics library could not be loaded.
    pub missing_32bit_graphics: bool,
    /// Name resolution or connection attempts failed.
    pub network_failures: bool,
}

/// Scan log lines (the tail of `game.log`).
pub fn scan_log(text: &str) -> LogFindings {
    let mut f = LogFindings::default();
    for line in text.lines() {
        let l = line.to_ascii_lowercase();
        if (l.contains("permission denied")
            || l.contains("access denied")
            || l.contains("status_access_denied")
            || l.contains("read-only file system")
            || l.contains("erofs")
            || l.contains("c0000022"))
            && !l.contains("/proc/")
            && !l.contains("/sys/")
        {
            f.access_denied_writes = true;
        }
        if l.contains("wrong elf class")
            || l.contains("i386-linux-gnu")
            || (l.contains("32-bit") && (l.contains("driver") || l.contains("libgl")))
            || l.contains("err:vulkan:wine_vk_init")
            || l.contains("libvulkan.so.1: cannot open")
            || l.contains("libgl.so.1: cannot open")
            || (l.contains("no icd") && l.contains("vulkan"))
        {
            f.missing_32bit_graphics = true;
        }
        if l.contains("could not resolve host")
            || l.contains("getaddrinfo")
            || l.contains("name or service not known")
            || l.contains("network is unreachable")
            || l.contains("wsaenetunreach")
            || (l.contains("winhttp") && l.contains("err:"))
        {
            f.network_failures = true;
        }
    }
    f
}

/// Everything the suggestions are derived from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Evidence {
    /// The classified error of a launch that failed before the game ran.
    pub known_error: Option<ErrorCode>,
    /// The game ran without network access.
    pub offline: bool,
    /// The program probably uses the network (setup hints, or network failures in the log).
    pub online_capable: bool,
    /// How the game folder was visible.
    pub game_dir: Option<GameDirAccess>,
    /// The preset the game ran with.
    pub preset: Option<BasePreset>,
    /// The program is 32-bit.
    pub is_32bit: bool,
    /// What the log showed.
    pub log: LogFindings,
}

impl Evidence {
    /// Fill `offline` from the session's network mode.
    pub fn with_network(mut self, network: Option<NetworkAccess>) -> Self {
        self.offline = network == Some(NetworkAccess::None);
        self
    }
}

/// Whether a known error is specific enough that its catalogue actions come first.
fn specific(code: ErrorCode) -> bool {
    matches!(
        code.area(),
        "UMU" | "SBX" | "RT" | "HOST" | "SES" | "LIB" | "FS"
    )
}

/// The ordered suggestions of UF-9 (without the always-available actions).
pub fn suggestions(e: &Evidence) -> Vec<RecoveryAction> {
    use RecoveryAction as A;
    let mut out: Vec<RecoveryAction> = Vec::new();
    let mut push = |a: RecoveryAction| {
        if !out.contains(&a) && !matches!(a, A::Retry | A::ViewLog | A::CopyDetails | A::Dismiss) {
            out.push(a);
        }
    };
    let mut found = false;
    if let Some(code) = e.known_error.filter(|c| specific(*c)) {
        for a in crate::error::catalog::entry(code).actions {
            push(*a);
        }
        found = true;
    }
    if e.offline && (e.online_capable || e.log.network_failures) {
        push(A::SwitchPreset(BasePreset::Online));
        found = true;
    }
    if matches!(
        e.game_dir,
        Some(GameDirAccess::ReadOnly | GameDirAccess::FileOnly)
    ) && e.log.access_denied_writes
    {
        push(A::AllowGameFolderWrites);
        push(A::ChooseGameFolder);
        found = true;
    }
    if e.is_32bit && e.log.missing_32bit_graphics {
        push(A::OpenDoctor);
        found = true;
    }
    if !found {
        push(A::ChangeRuntime);
        if e.preset != Some(BasePreset::Standard) {
            push(A::SwitchPreset(BasePreset::Standard));
        }
        push(A::ResetPrefix);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use RecoveryAction as A;

    /// ERR-006 acceptance: each evidence row of UF-9 produces its first suggestion.
    #[test]
    fn evidence_table() {
        let none = Evidence::default();
        let rows: Vec<(&str, Evidence, RecoveryAction)> = vec![
            (
                "known container error",
                Evidence {
                    known_error: Some(ErrorCode::UmuContainerFailed),
                    ..none
                },
                A::OpenDoctor,
            ),
            (
                "known sandbox error",
                Evidence {
                    known_error: Some(ErrorCode::SbxGrantMissing),
                    ..none
                },
                A::EditPermissions,
            ),
            (
                "offline and online-capable",
                Evidence {
                    offline: true,
                    online_capable: true,
                    ..none
                },
                A::SwitchPreset(BasePreset::Online),
            ),
            (
                "offline and network failures in the log",
                Evidence {
                    offline: true,
                    log: scan_log("err:winhttp:x Could not resolve host: api.example"),
                    ..none
                },
                A::SwitchPreset(BasePreset::Online),
            ),
            (
                "read-only folder and access denied",
                Evidence {
                    game_dir: Some(GameDirAccess::ReadOnly),
                    log: scan_log("[+1.2] Unity: Permission denied writing Player.log"),
                    ..none
                },
                A::AllowGameFolderWrites,
            ),
            (
                "file-only and read-only file system",
                Evidence {
                    game_dir: Some(GameDirAccess::FileOnly),
                    log: scan_log("err:file:x Read-only file system"),
                    ..none
                },
                A::AllowGameFolderWrites,
            ),
            (
                "32-bit drivers",
                Evidence {
                    is_32bit: true,
                    log: scan_log("libGL error: wrong ELF class: ELFCLASS64 (/usr/lib/libGL.so.1)"),
                    ..none
                },
                A::OpenDoctor,
            ),
            ("nothing specific", none, A::ChangeRuntime),
        ];
        for (name, e, first) in rows {
            let s = suggestions(&e);
            assert_eq!(s.first(), Some(&first), "{name}: {s:?}");
            assert!(
                !s.iter()
                    .any(|a| matches!(a, A::Retry | A::ViewLog | A::CopyDetails)),
                "{name}"
            );
        }
        // Generic fallbacks, Standard dropped when already Standard.
        assert_eq!(
            suggestions(&none),
            vec![
                A::ChangeRuntime,
                A::SwitchPreset(BasePreset::Standard),
                A::ResetPrefix
            ]
        );
        let std = Evidence {
            preset: Some(BasePreset::Standard),
            ..none
        };
        assert_eq!(suggestions(&std), vec![A::ChangeRuntime, A::ResetPrefix]);
        // Writable folder: access-denied lines are not about the folder.
        let rw = Evidence {
            game_dir: Some(GameDirAccess::ReadWrite),
            log: scan_log("Permission denied"),
            ..none
        };
        assert_eq!(suggestions(&rw).first(), Some(&A::ChangeRuntime));
        // Online but not offline: no switch.
        let online = Evidence {
            online_capable: true,
            ..none
        };
        assert_eq!(suggestions(&online).first(), Some(&A::ChangeRuntime));
    }

    #[test]
    fn log_scan_ignores_proc_noise() {
        let f = scan_log("open /proc/1/environ: Permission denied");
        assert!(!f.access_denied_writes);
    }
}
