//! Provision and Prepare: which trusted step a launch needs, and what a finished trusted
//! session means (RUNTIME_MANAGEMENT §4, SECURITY_MODEL §4.1; RT-009, RT-010, RT-012).
//!
//! - [`next_step`] is the first-launch decision table of §4.3 with the pristine-prefix rule:
//!   a network-enabled Prepare may mount a prefix only while it is pristine, unless the user
//!   allowed the network once ([`StepInput::consent`]) or the game's own policy has it.
//! - [`interpret`] turns the end of a Provision or Prepare session into success or the
//!   user-facing error (`ZN-RT-010` when the network is missing).
//! - [`check_postconditions`] and [`sweep_partial`] are the post-conditions and the
//!   post-failure sweep of §4.1 and §9.
//! - [`CacheMeter`] measures download progress from umu's cache (bytes and rate).
//!
//! The sessions themselves are planned by `launch::planner` (kinds `Provision`/`Prepare`)
//! and run by `launch::trusted`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::catalog::RuntimeCatalog;
use super::discovery::detect_slr;
use super::resolver::{ProvisionNeed, Resolution, UnavailableReason};
use super::store::RuntimeStore;
use crate::clock::MonoInstant;
use crate::error::{Error, ErrorCode};
use crate::fsx::{FileType, Fs, safe_remove_tree};
use crate::model::{EndReason, NetworkAccess, RuntimeOrigin, SessionKind, SlrVariant};

/// Time a Prepare session may take (LOGIC_SPECS §3).
pub const PREPARE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Grace period when the fast path stops a Provision session (§4.1).
pub const FAST_PATH_GRACE: Duration = Duration::from_secs(3);
/// Interval of the cache size poll (§4.1, RT-012).
pub const CACHE_POLL: Duration = Duration::from_millis(500);

/// The state of a game's prefix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefixState {
    /// `drive_c` exists.
    pub exists: bool,
    /// An untrusted session ran in it (`games.prefix_tainted_at`).
    pub tainted: bool,
}

impl PrefixState {
    /// Read the state of `prefix` (`drive_c` is a directory) with the stored taint.
    pub fn read(fs: &dyn Fs, prefix: &Path, tainted: bool) -> Self {
        Self {
            exists: fs
                .metadata(&prefix.join("drive_c"))
                .is_ok_and(|m| m.is_dir()),
            tainted,
        }
    }
}

/// Inputs of [`next_step`].
#[derive(Clone, Copy, Debug)]
pub struct StepInput<'a> {
    /// The current resolution.
    pub resolution: &'a Resolution,
    /// The prefix.
    pub prefix: PrefixState,
    /// Preference `protection.prepare_online`.
    pub prepare_online: bool,
    /// The game's own network policy.
    pub game_network: NetworkAccess,
    /// The user allowed the network once for this launch (prompt `FixNeedsNetwork`).
    pub consent: bool,
    /// For `NeedsProvision(Slr)`: the build that needs the runtime is external (umu cannot
    /// find it by name; a Provision session with its path installs the runtime).
    pub slr_build_external: bool,
}

/// What the launch does next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextStep {
    /// Everything is in place: the game session.
    Game,
    /// A Provision session (no game data mounted).
    Provision(ProvisionNeed),
    /// A Prepare session for the game's prefix.
    Prepare {
        /// Network of the session.
        network: NetworkAccess,
        /// `PROTONPATH` name to download, when the build is missing (network on only).
        codename: Option<String>,
    },
    /// The reference cannot be satisfied.
    Unavailable(UnavailableReason),
}

/// Network of a Prepare session (§4.2, SECURITY_MODEL §4.1).
pub fn prepare_network(i: &StepInput<'_>) -> NetworkAccess {
    // Rule 1: the network may meet a prefix only while it is pristine; rule 2: a tainted
    // prefix gets it only with the user's consent (or its own policy).
    let pristine_online = !i.prefix.tainted && i.prepare_online;
    let consented = i.prefix.tainted && i.consent;
    if pristine_online || consented {
        NetworkAccess::Full
    } else {
        i.game_network
    }
}

/// The decision table of RUNTIME_MANAGEMENT §4.3.
pub fn next_step(i: &StepInput<'_>) -> NextStep {
    match i.resolution {
        Resolution::Unavailable(r) => NextStep::Unavailable(r.clone()),
        Resolution::Ready(_) if i.prefix.exists => NextStep::Game,
        Resolution::Ready(_) => NextStep::Prepare {
            network: prepare_network(i),
            codename: None,
        },
        Resolution::NeedsProvision(need) => {
            let by_name = !matches!(need, ProvisionNeed::Slr { .. }) || !i.slr_build_external;
            if !i.prefix.tainted && i.prepare_online && by_name {
                // One step for the common first run: Prepare downloads what is missing.
                NextStep::Prepare {
                    network: NetworkAccess::Full,
                    codename: Some(need.protonpath()),
                }
            } else {
                NextStep::Provision(need.clone())
            }
        }
    }
}

/// How a trusted session ended (from its row and log).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrustedEnd {
    /// Session kind (`Provision` or `Prepare`).
    pub kind: SessionKind,
    /// The row's end reason.
    pub reason: EndReason,
    /// umu reported `Launching` (installation finished).
    pub launching_seen: bool,
    /// The session stopped because of the fast path.
    pub fast_path_stopped: bool,
    /// The user cancelled.
    pub cancelled: bool,
    /// The log shows a failed connection.
    pub network_failure: bool,
    /// The log shows a download cut short (timeout, reset, HTTP error); umu resumes it.
    pub download_interrupted: bool,
    /// After a Prepare: the prefix has `drive_c` and `system.reg`.
    pub prefix_ready: bool,
}

/// Success, or the error to show (§9 failure table, LOGIC_SPECS §3).
pub fn interpret(end: &TrustedEnd) -> Result<(), Error> {
    if end.cancelled {
        return Err(Error::new(ErrorCode::IntTaskFailed).with("reason", "cancelled"));
    }
    let offline = || Error::new(ErrorCode::RtNotInstalledOffline);
    match (end.kind, end.reason) {
        (SessionKind::Provision, _) if end.launching_seen => Ok(()),
        // `umu-run ""` creates the prefix and then fails to start the empty program
        // ("ShellExecuteEx failed: File not found", exit 1): the prefix is what counts.
        (SessionKind::Prepare, _) if end.launching_seen && end.prefix_ready => Ok(()),
        (_, EndReason::Exited { code: 0 }) => Ok(()),
        (
            _,
            EndReason::FailedToStart {
                code: ErrorCode::RtNotInstalledOffline,
            },
        ) => Err(offline()),
        _ if end.network_failure => Err(offline()),
        _ if end.download_interrupted => Err(Error::new(ErrorCode::RtDownloadInterrupted)),
        (_, EndReason::FailedToStart { code }) => Err(Error::new(code)),
        (SessionKind::Provision, EndReason::Exited { code }) => {
            Err(Error::new(ErrorCode::RtDownloadInterrupted).with("exit_status", code))
        }
        (SessionKind::Provision, r) => {
            Err(Error::new(ErrorCode::RtDownloadInterrupted).with("end", r.as_str()))
        }
        (_, EndReason::Exited { code }) => {
            Err(Error::new(ErrorCode::RtPrefixPrepareFailed).with("exit_status", code))
        }
        (_, r) => Err(Error::new(ErrorCode::RtPrefixPrepareFailed).with("end", r.as_str())),
    }
}

/// Whether `text` (the tail of a trusted session's log) shows a missing connection.
pub fn log_shows_offline(text: &str) -> bool {
    text.lines().any(|l| {
        crate::umu::log_parse::is_network_failure(l)
            || l.contains("An internet connection is required")
            || l.contains("Temporary failure in name resolution")
    })
}

/// Whether the log tail shows a download that was cut short (§9 "Download interrupted").
pub fn log_shows_interrupted(text: &str) -> bool {
    text.lines().any(|l| {
        [
            "Read timed out",
            "ReadTimeoutError",
            "TimeoutError",
            "IncompleteRead",
            "Connection reset",
            "ProtocolError",
            "HTTPError",
        ]
        .iter()
        .any(|p| l.contains(p))
    })
}

/// Check §4.1's post-conditions for `need` on a fresh catalogue: the build exists with
/// `proton` and `toolmanifest.vdf` (validated by discovery), and its runtime variant is
/// complete. Returns the variants that are now installed.
pub fn check_postconditions(
    catalog: &RuntimeCatalog,
    need: &ProvisionNeed,
) -> Result<Vec<SlrVariant>, Error> {
    let failed = |what: String| {
        Err(Error::new(ErrorCode::RtNotInstalled)
            .with("protonpath", need.protonpath())
            .with("detail", what))
    };
    let build = match need {
        ProvisionNeed::Codename(c) => catalog
            .builds
            .iter()
            .filter(|b| b.family == c.family() && b.origin == RuntimeOrigin::Store)
            .max_by(|a, b| super::store::compare_builds(a.name.as_str(), b.name.as_str())),
        ProvisionNeed::Exact(n) | ProvisionNeed::Slr { build: n, .. } => catalog.by_name(n),
    };
    let Some(build) = build else {
        return failed("the compatibility layer was not installed".into());
    };
    let variant = build.required_slr;
    match catalog.slr(variant) {
        Some(s) if s.is_complete() && s.platform_dir.is_some() => {}
        _ => return failed(format!("the runtime {} is incomplete", variant.as_str())),
    }
    Ok(catalog
        .slr
        .iter()
        .filter(|s| s.is_complete())
        .map(|s| s.variant)
        .collect())
}

/// §9 "Disk full": remove what a failed install left in the store — build directories
/// without `proton`, variant directories without the install marker, adoption staging
/// directories. Never touches anything outside the store. Run only while holding the
/// store lock and with no session using the store.
pub fn sweep_partial(fs: &dyn Fs, store: &RuntimeStore) -> Vec<PathBuf> {
    let mut removed = Vec::new();
    let mut remove = |root: &Path, p: PathBuf| match safe_remove_tree(fs, root, &p) {
        Ok(_) => removed.push(p),
        Err(e) => tracing::warn!(path = %p.display(), error = %e, "partial install not removed"),
    };
    if let Ok(entries) = fs.read_dir(&store.compat) {
        for e in entries {
            if e.file_type == FileType::Dir && !fs.exists(&e.path.join("proton")) {
                remove(&store.compat, e.path);
            }
        }
    }
    for v in [SlrVariant::Steamrt3, SlrVariant::Steamrt4] {
        let dir = store.slr_dir(v);
        if fs.metadata(&dir).is_ok_and(|m| m.is_dir())
            && detect_slr(fs, &dir, v).is_none_or(|s| !s.is_complete())
        {
            remove(&store.umu, dir);
        }
    }
    if let Ok(entries) = fs.read_dir(&store.root) {
        for e in entries {
            if e.file_type == FileType::Dir && e.name.to_string_lossy().starts_with(".adopt-") {
                remove(&store.root, e.path);
            }
        }
    }
    removed
}

/// Keep interrupted downloads resumable (§4.1 "Cancel", F-126): umu 1.4 downloads into
/// `tmpXXXXXXXX` directories of its cache and resumes only from `<cache>/<name>.parts`; a
/// session stopped by a signal leaves the part in the temporary directory. Move such parts
/// back (never over an existing file) and remove the emptied directories. Returns the
/// parts moved. Only for umu's cache, which only trusted sessions write.
pub fn rescue_parts(fs: &dyn Fs, cache: &Path) -> Vec<PathBuf> {
    let mut moved = Vec::new();
    let Ok(entries) = fs.read_dir(cache) else {
        return moved;
    };
    for e in entries {
        if e.file_type != FileType::Dir || !e.name.to_string_lossy().starts_with("tmp") {
            continue;
        }
        let Ok(inner) = fs.read_dir(&e.path) else {
            continue;
        };
        for f in inner {
            let name = f.name.to_string_lossy().into_owned();
            let to = cache.join(&name);
            if f.file_type == FileType::File
                && name.ends_with(".parts")
                && !fs.exists(&to)
                && fs.rename(&f.path, &to).is_ok()
            {
                moved.push(to);
            }
        }
        let _ = safe_remove_tree(fs, cache, &e.path);
    }
    moved
}

/// Download progress from umu's cache (RT-012): the total size of `*.parts` files and of
/// temporary directories (hidden ones, and umu 1.4's `tmpXXXXXXXX`), and the transfer rate
/// since the previous poll.
#[derive(Debug)]
pub struct CacheMeter {
    dir: PathBuf,
    last: Option<(MonoInstant, u64)>,
    rate: u64,
}

/// Entries a single poll looks at (umu's cache holds a handful of files).
const METER_ENTRY_CAP: usize = 4096;

impl CacheMeter {
    /// A meter over `<cache>/umu`.
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            last: None,
            rate: 0,
        }
    }

    fn tree_size(fs: &dyn Fs, dir: &Path, budget: &mut usize, depth: usize) -> u64 {
        let Ok(entries) = fs.read_dir(dir) else {
            return 0;
        };
        let mut total = 0;
        for e in entries {
            if *budget == 0 {
                break;
            }
            *budget -= 1;
            match e.file_type {
                FileType::File => total += fs.metadata(&e.path).map_or(0, |m| m.size),
                FileType::Dir if depth < 8 => {
                    total += Self::tree_size(fs, &e.path, budget, depth + 1);
                }
                _ => {}
            }
        }
        total
    }

    /// Bytes in transit now.
    pub fn bytes(&self, fs: &dyn Fs) -> u64 {
        let Ok(entries) = fs.read_dir(&self.dir) else {
            return 0;
        };
        let mut budget = METER_ENTRY_CAP;
        let mut total = 0;
        for e in entries {
            let name = e.name.to_string_lossy();
            match e.file_type {
                FileType::File if name.ends_with(".parts") => {
                    total += fs.metadata(&e.path).map_or(0, |m| m.size);
                }
                // umu downloads into `tempfile` directories (`tmpXXXXXXXX`) of its cache.
                FileType::Dir if name.starts_with('.') || name.starts_with("tmp") => {
                    total += Self::tree_size(fs, &e.path, &mut budget, 0);
                }
                _ => {}
            }
        }
        total
    }

    /// Poll at `now`: `(bytes, bytes per second)`.
    pub fn poll(&mut self, fs: &dyn Fs, now: MonoInstant) -> (u64, u64) {
        let bytes = self.bytes(fs);
        if let Some((t, b)) = self.last {
            let dt = now.saturating_duration_since(t).as_millis();
            let delta = u128::from(bytes.saturating_sub(b));
            if let Some(r) = (delta * 1000).checked_div(dt) {
                self.rate = u64::try_from(r).unwrap_or(u64::MAX);
            }
        }
        self.last = Some((now, bytes));
        (bytes, self.rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::RuntimeName;
    use crate::model::Channel;

    fn input(r: &Resolution, exists: bool, tainted: bool, online: bool) -> StepInput<'_> {
        StepInput {
            resolution: r,
            prefix: PrefixState { exists, tainted },
            prepare_online: online,
            game_network: NetworkAccess::None,
            consent: false,
            slr_build_external: false,
        }
    }

    #[test]
    fn first_launch_table() {
        let need = Resolution::NeedsProvision(ProvisionNeed::Codename(Channel::UmuProton));
        // Pristine, online prepare allowed: Prepare with the codename.
        assert_eq!(
            next_step(&input(&need, false, false, true)),
            NextStep::Prepare {
                network: NetworkAccess::Full,
                codename: Some("UMU-Proton".into())
            }
        );
        // Tainted: Provision (never the prefix with the network).
        assert_eq!(
            next_step(&input(&need, true, true, true)),
            NextStep::Provision(ProvisionNeed::Codename(Channel::UmuProton))
        );
        // Online prepare off: Provision first.
        assert!(matches!(
            next_step(&input(&need, false, false, false)),
            NextStep::Provision(_)
        ));
        // External build needing a runtime: Provision with its path.
        let slr = Resolution::NeedsProvision(ProvisionNeed::Slr {
            variant: SlrVariant::Steamrt4,
            build: RuntimeName::new("proton-cachyos-11.0").unwrap(),
        });
        let mut i = input(&slr, false, false, true);
        i.slr_build_external = true;
        assert!(matches!(next_step(&i), NextStep::Provision(_)));
    }

    #[test]
    fn interpretation() {
        let end = |kind, reason| TrustedEnd {
            kind,
            reason,
            launching_seen: false,
            fast_path_stopped: false,
            cancelled: false,
            network_failure: false,
            download_interrupted: false,
            prefix_ready: false,
        };
        let mut e = end(SessionKind::Provision, EndReason::StoppedByUser);
        e.launching_seen = true;
        assert!(interpret(&e).is_ok());
        let e = end(
            SessionKind::Provision,
            EndReason::FailedToStart {
                code: ErrorCode::RtNotInstalledOffline,
            },
        );
        assert_eq!(
            interpret(&e).unwrap_err().code,
            ErrorCode::RtNotInstalledOffline
        );
        let mut e = end(SessionKind::Prepare, EndReason::Exited { code: 1 });
        assert_eq!(
            interpret(&e).unwrap_err().code,
            ErrorCode::RtPrefixPrepareFailed
        );
        e.network_failure = true;
        assert_eq!(
            interpret(&e).unwrap_err().code,
            ErrorCode::RtNotInstalledOffline
        );
        assert!(interpret(&end(SessionKind::Prepare, EndReason::Exited { code: 0 })).is_ok());
        let mut e = end(
            SessionKind::Prepare,
            EndReason::FailedToStart {
                code: ErrorCode::UmuInternalError,
            },
        );
        e.launching_seen = true;
        assert!(interpret(&e).is_err());
        e.prefix_ready = true;
        assert!(interpret(&e).is_ok());
        let mut e = end(
            SessionKind::Provision,
            EndReason::FailedToStart {
                code: ErrorCode::RtUnresolvable,
            },
        );
        e.download_interrupted = log_shows_interrupted(
            "urllib3.exceptions.ReadTimeoutError: HTTPSConnectionPool(host='x', port=443): Read timed out.",
        );
        assert_eq!(
            interpret(&e).unwrap_err().code,
            ErrorCode::RtDownloadInterrupted
        );
        assert!(log_shows_offline(
            "urllib.error.URLError: <urlopen error [Errno -3] Temporary failure in name resolution>"
        ));
    }
}
