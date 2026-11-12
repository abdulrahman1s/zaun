//! Detecting missing games (LOGIC_SPECS §9): one `stat` per primary executable, a deadline
//! per mount point (a hanging network mount delays only its own games), classification into
//! [`Availability`], and the trigger rate limit. Nothing is written to the database and no
//! directory is walked.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::RecvTimeoutError;

use super::index::LibrarySnapshot;
use crate::clock::{Clock, MonoInstant};
use crate::fsx::{Fs, FsErrorKind, MountTable};
use crate::ids::GameId;
use crate::model::{Availability, ExeRole, Game};

/// Deadline of one mount point's `stat`s.
pub const MOUNT_DEADLINE: Duration = Duration::from_secs(2);
/// "Main window gained focus" scans at most this often.
pub const FOCUS_MIN_INTERVAL: Duration = Duration::from_secs(60);

/// One game to check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanTarget {
    /// The game.
    pub game: GameId,
    /// Its primary executable (canonical path).
    pub path: PathBuf,
    /// The stored mount point (`executables.mount_point`).
    pub mount_point: Option<PathBuf>,
}

impl ScanTarget {
    /// The target for a game's primary executable.
    pub fn of(game: &Game) -> Option<Self> {
        let e = game
            .executables
            .iter()
            .find(|e| e.role == ExeRole::Primary)?;
        Some(Self {
            game: game.id,
            path: e.canonical_path.clone(),
            mount_point: e.mount_point.clone(),
        })
    }
}

/// Targets for every game of a snapshot, given the stored mount points by game.
pub fn targets_from_snapshot(
    snapshot: &LibrarySnapshot,
    mount_points: &BTreeMap<GameId, Option<PathBuf>>,
) -> Vec<ScanTarget> {
    snapshot
        .games
        .iter()
        .map(|g| ScanTarget {
            game: g.id,
            path: g.primary_exe_path.to_path_buf(),
            mount_point: mount_points.get(&g.id).cloned().flatten(),
        })
        .collect()
}

/// Where removable media are mounted for `user`.
pub fn media_roots(user: &str) -> Vec<PathBuf> {
    vec![
        Path::new("/run/media").join(user),
        Path::new("/media").join(user),
        PathBuf::from("/mnt"),
    ]
}

fn is_mounted(mounts: &MountTable, point: &Path) -> bool {
    point == Path::new("/") || mounts.mounts.iter().any(|m| m.mount_point == point)
}

/// Classify one executable (blocking; run under a deadline by [`scan`]).
pub fn classify(
    fs: &dyn Fs,
    mounts: &MountTable,
    media: &[PathBuf],
    t: &ScanTarget,
) -> Availability {
    match fs.metadata_follow(&t.path) {
        Ok(md) if md.is_file() => return Availability::Present,
        Ok(_) => {}
        Err(e) => match e.kind {
            FsErrorKind::PermissionDenied | FsErrorKind::TimedOut => {
                return Availability::Unreadable;
            }
            FsErrorKind::Other => return Availability::Unreadable,
            _ => {}
        },
    }
    if let Some(mp) = &t.mount_point
        && !is_mounted(mounts, mp)
    {
        return Availability::DriveDisconnected;
    }
    // Nearest existing ancestor.
    let mut cur = t.path.parent();
    while let Some(dir) = cur {
        if fs.metadata(dir).is_ok() {
            if media.iter().any(|m| m == dir) {
                return Availability::DriveDisconnected;
            }
            break;
        }
        cur = dir.parent();
    }
    Availability::Missing
}

/// Group key: the stored mount point, else the mount containing the path.
fn group_of(mounts: &MountTable, t: &ScanTarget) -> PathBuf {
    t.mount_point.clone().unwrap_or_else(|| {
        mounts
            .mount_point_of(&t.path)
            .map_or_else(|| PathBuf::from("/"), |m| m.mount_point.clone())
    })
}

/// Classify every target. Targets are grouped by mount point; each group runs on its own
/// thread and gets `deadline` from the start of the scan; a group that does not answer in
/// time leaves its remaining games `Unreadable` (its thread is abandoned, not joined: a
/// hung `stat` cannot be cancelled). Results are in target order.
pub fn scan(
    fs: Arc<dyn Fs>,
    clock: &dyn Clock,
    mounts: &MountTable,
    media: &[PathBuf],
    targets: &[ScanTarget],
    deadline: Duration,
) -> Vec<(GameId, Availability)> {
    let mut groups: BTreeMap<PathBuf, Vec<(usize, ScanTarget)>> = BTreeMap::new();
    for (i, t) in targets.iter().enumerate() {
        groups
            .entry(group_of(mounts, t))
            .or_default()
            .push((i, t.clone()));
    }
    let (tx, rx) = crossbeam_channel::unbounded::<(usize, Availability)>();
    for (_, group) in groups {
        let fs = fs.clone();
        let mounts = mounts.clone();
        let media = media.to_vec();
        let tx = tx.clone();
        let spawned = std::thread::Builder::new()
            .name("availability".into())
            .spawn(move || {
                for (i, t) in group {
                    if tx.send((i, classify(&*fs, &mounts, &media, &t))).is_err() {
                        return;
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "availability thread not started");
        }
    }
    drop(tx);
    let mut out: Vec<Option<Availability>> = vec![None; targets.len()];
    let until: MonoInstant = clock.mono().saturating_add(deadline);
    let mut remaining = targets.len();
    while remaining > 0 {
        let left = until.saturating_duration_since(clock.mono());
        match rx.recv_timeout(left) {
            Ok((i, a)) => {
                if out[i].is_none() {
                    remaining -= 1;
                }
                out[i] = Some(a);
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
        }
    }
    targets
        .iter()
        .zip(out)
        .map(|(t, a)| (t.game, a.unwrap_or(Availability::Unreadable)))
        .collect()
}

/// Why a scan is requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScanTrigger {
    /// Start-up.
    Startup,
    /// The main window gained focus.
    Focus,
    /// `RescanMissing` ("Check again").
    Manual,
    /// Before launching one game.
    BeforeLaunch,
}

/// The focus-trigger rate limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanThrottle {
    last: Option<MonoInstant>,
}

impl ScanThrottle {
    /// Whether to scan now; records the scan when it says yes.
    pub fn should_scan(&mut self, trigger: ScanTrigger, now: MonoInstant) -> bool {
        let ok = match (trigger, self.last) {
            (ScanTrigger::Focus, Some(last)) => {
                now.saturating_duration_since(last) >= FOCUS_MIN_INTERVAL
            }
            _ => true,
        };
        if ok && trigger != ScanTrigger::BeforeLaunch {
            self.last = Some(now);
        }
        ok
    }
}
