//! Play time from Steam (ADR-0031 §3, STATISTICS §11): `config/loginusers.vdf` names the
//! accounts that signed in on this computer; the most recently used one's
//! `userdata/<account id>/config/localconfig.vdf` holds `Playtime` (minutes) and
//! `LastPlayed` (Unix seconds) per app under `UserLocalConfigStore/Software/Valve/Steam/apps`.
//!
//! Nothing about the account (names, IDs) is kept, logged or shown; only the per-app numbers
//! are used.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::keyvalues::{KvLimits, KvObject, parse};
use crate::clock::Timestamp;
use crate::fsx::{FileType, Fs};

/// Largest `loginusers.vdf` read.
pub const LOGINUSERS_MAX: u64 = 1024 * 1024;
/// Largest `localconfig.vdf` read (hundreds of kilobytes on a real machine).
pub const LOCALCONFIG_MAX: u64 = 32 * 1024 * 1024;

/// Lowest 64-bit Steam ID of an individual account; the account ID is the low 32 bits.
const STEAMID64_BASE: u64 = 76_561_197_960_265_728;

/// What Steam recorded for one app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AppPlay {
    /// Total play time in minutes.
    pub minutes: u64,
    /// Last played.
    pub last_played: Option<Timestamp>,
}

/// The account ID (the `userdata` folder name) of the most recently used account in
/// `loginusers.vdf`: the one marked `MostRecent`, else the highest `Timestamp`.
pub fn most_recent_account(bytes: &[u8]) -> Option<u32> {
    let parsed = parse(bytes, &KvLimits::SMALL);
    let users = parsed.root.obj("users")?;
    let mut best: Option<(bool, u64, u32)> = None;
    for (key, user) in users.objects() {
        let Some(id64) = key
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|v| *v >= STEAMID64_BASE)
        else {
            continue;
        };
        let account = (id64 & 0xFFFF_FFFF) as u32;
        if account == 0 {
            continue;
        }
        let recent = user.str("MostRecent").map(str::trim) == Some("1");
        let ts = user.u64("Timestamp").unwrap_or(0);
        let cand = (recent, ts, account);
        if best.is_none_or(|b| (cand.0, cand.1) > (b.0, b.1)) {
            best = Some(cand);
        }
    }
    best.map(|b| b.2)
}

fn seconds(o: &KvObject, key: &str) -> Option<Timestamp> {
    o.u64(key)
        .filter(|s| *s > 0)
        .and_then(|s| i64::try_from(s).ok())
        .and_then(|s| s.checked_mul(1000))
        .map(Timestamp::from_millis)
}

/// Per-app play time from `localconfig.vdf` text.
pub fn play_times(bytes: &[u8]) -> HashMap<u32, AppPlay> {
    let parsed = parse(bytes, &KvLimits::LARGE);
    let Some(apps) =
        parsed
            .root
            .path(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"])
    else {
        return HashMap::new();
    };
    apps.objects()
        .filter_map(|(k, o)| {
            let id = k.trim().parse::<u32>().ok().filter(|v| *v > 0)?;
            let minutes = o.u64("Playtime");
            let last_played = seconds(o, "LastPlayed");
            (minutes.is_some() || last_played.is_some()).then_some((
                id,
                AppPlay {
                    minutes: minutes.unwrap_or(0),
                    last_played,
                },
            ))
        })
        .collect()
}

/// `localconfig.vdf` of the most recently used account under `root`: through
/// `loginusers.vdf`, else the `userdata/<numeric>` folder whose `localconfig.vdf` changed
/// last.
pub fn localconfig_path(fs: &dyn Fs, root: &Path) -> Option<PathBuf> {
    let file = |id: u32| {
        root.join("userdata")
            .join(id.to_string())
            .join("config/localconfig.vdf")
    };
    if let Some(id) = fs
        .read(&root.join("config/loginusers.vdf"), LOGINUSERS_MAX)
        .ok()
        .and_then(|b| most_recent_account(&b))
        && fs.metadata(&file(id)).is_ok_and(|m| m.is_file())
    {
        return Some(file(id));
    }
    let entries = fs.read_dir(&root.join("userdata")).ok()?;
    entries
        .into_iter()
        .filter(|e| e.file_type == FileType::Dir)
        .filter_map(|e| e.name.to_str()?.parse::<u32>().ok().filter(|v| *v > 0))
        .filter_map(|id| {
            let p = file(id);
            let md = fs.metadata(&p).ok().filter(|m| m.is_file())?;
            Some((md.mtime_ns, p))
        })
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

/// The play times of the most recently used account under `root` (empty when unknown).
pub fn read_play_times(fs: &dyn Fs, root: &Path) -> HashMap<u32, AppPlay> {
    localconfig_path(fs, root)
        .and_then(|p| fs.read(&p, LOCALCONFIG_MAX).ok())
        .map(|b| play_times(&b))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_recent_by_flag_then_timestamp() {
        let users = include_bytes!("../../tests/data/steam/loginusers.vdf");
        // 76561198000000001 − 76561197960265728 = 39734273 (its low 32 bits).
        assert_eq!(most_recent_account(users), Some(39_734_273));
        let flagged = b"\"users\" { \"76561198000000001\" { \"Timestamp\" \"9\" } \
                        \"76561198000000002\" { \"MostRecent\" \"1\" \"Timestamp\" \"1\" } }";
        assert_eq!(most_recent_account(flagged), Some(39_734_274));
        assert_eq!(most_recent_account(b"\"users\" { \"12\" { } }"), None);
        assert_eq!(most_recent_account(b""), None);
    }

    #[test]
    fn play_times_per_app() {
        let t = play_times(include_bytes!("../../tests/data/steam/localconfig.vdf"));
        assert_eq!(
            t[&1_174_180],
            AppPlay {
                minutes: 4474,
                last_played: Some(Timestamp::from_millis(1_790_786_743_000)),
            }
        );
        assert_eq!(t[&440].minutes, 125);
        assert_eq!(
            t[&12210],
            AppPlay {
                minutes: 0,
                last_played: None
            }
        );
        assert!(play_times(b"\"UserLocalConfigStore\" { }").is_empty());
    }
}
