//! Proton use and the game's Windows storage (ADR-0031 §3, §6).
//!
//! - `config/config.vdf` → `InstallConfigStore/Software/Valve/Steam/CompatToolMapping/<appid>`
//!   `name`: the tool the user chose for the game (`"0"` is Steam's default for all games).
//! - `steamapps/compatdata/<appid>` in the library folder that holds the game (Steam's
//!   default library as a fallback): the compatibility data; `pfx` inside it is the prefix.
//! - `compatdata/<appid>/config_info`: line 2 is `<proton>/files/share/fonts/` (or
//!   `<proton>/dist/share/fonts/` for older builds) of the Proton that last ran the prefix;
//!   `compatdata/<appid>/version` its version.
//!
//! The tool mapping's names are Steam's internal names, which are not folder names
//! (`proton_9` is `steamapps/common/Proton 9.0`); the folder that runs a program in the
//! prefix is taken from `config_info`, never guessed from the mapping.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::keyvalues::{KvLimits, parse};
use crate::fsx::Fs;
use crate::model::SteamCompat;

/// Largest `config.vdf` read.
pub const CONFIG_MAX: u64 = 32 * 1024 * 1024;
/// Largest `config_info` / `version` read.
pub const SMALL_FILE_MAX: u64 = 64 * 1024;

/// The per-app tool names of `CompatToolMapping` (app ID 0, Steam's default, included).
pub fn tool_mapping(bytes: &[u8]) -> HashMap<u32, String> {
    let parsed = parse(bytes, &KvLimits::LARGE);
    let Some(map) = parsed.root.path(&[
        "InstallConfigStore",
        "Software",
        "Valve",
        "Steam",
        "CompatToolMapping",
    ]) else {
        return HashMap::new();
    };
    map.objects()
        .filter_map(|(k, o)| {
            let id = k.trim().parse::<u32>().ok()?;
            let name = o.str("name").map(str::trim).filter(|n| !n.is_empty())?;
            let name: String = name.chars().filter(|c| !c.is_control()).take(128).collect();
            Some((id, name))
        })
        .collect()
}

/// The Proton directory recorded in `config_info` text (line 2 without its
/// `/files/share/fonts/` or `/dist/share/fonts/` tail); `None` when the line has another
/// shape or is not an absolute path.
pub fn proton_dir_from_config_info(text: &str) -> Option<PathBuf> {
    let line = text.lines().nth(1)?.trim();
    let line = line.strip_suffix('/').unwrap_or(line);
    let dir = line
        .strip_suffix("/files/share/fonts")
        .or_else(|| line.strip_suffix("/dist/share/fonts"))?;
    (dir.starts_with('/') && !dir.split('/').any(|c| c == "..")).then(|| PathBuf::from(dir))
}

/// What `compat_data` says about Proton for app `app_id`: the mapped tool, the version and
/// Proton directory of the last run, the prefix. `mapping` is [`tool_mapping`]'s result.
pub fn read_compat(
    fs: &dyn Fs,
    app_id: u32,
    library: &Path,
    default_library: &Path,
    mapping: &HashMap<u32, String>,
) -> SteamCompat {
    let tool = mapping.get(&app_id).cloned();
    let rel = format!("steamapps/compatdata/{app_id}");
    let compat_data = [library.join(&rel), default_library.join(&rel)]
        .into_iter()
        .find(|p| fs.metadata(p).is_ok_and(|m| m.is_dir()));
    let Some(compat_data) = compat_data else {
        return match tool {
            Some(t) => SteamCompat::Proton {
                tool: Some(t),
                version: None,
                compat_data: library.join(&rel),
                prefix: None,
                proton_dir: None,
            },
            None => SteamCompat::NotProton,
        };
    };
    let pfx = compat_data.join("pfx");
    let prefix = fs.metadata(&pfx).is_ok_and(|m| m.is_dir()).then_some(pfx);
    let read_text = |name: &str| {
        fs.read(&compat_data.join(name), SMALL_FILE_MAX)
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    let version = read_text("version")
        .and_then(|t| t.lines().next().map(|l| l.trim().to_owned()))
        .filter(|v| !v.is_empty() && v.len() <= 128 && !v.chars().any(char::is_control));
    let proton_dir = read_text("config_info")
        .and_then(|t| proton_dir_from_config_info(&t))
        .filter(|d| {
            fs.metadata_follow(&d.join("proton"))
                .is_ok_and(|m| m.is_file())
        });
    SteamCompat::Proton {
        tool,
        version,
        compat_data,
        prefix,
        proton_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping() {
        let m = tool_mapping(include_bytes!("../../tests/data/steam/config.vdf"));
        assert_eq!(m.get(&0).map(String::as_str), Some("proton_experimental"));
        assert_eq!(
            m.get(&1_174_180).map(String::as_str),
            Some("GE-Proton10-25")
        );
        assert_eq!(m.len(), 2);
        assert!(tool_mapping(b"").is_empty());
    }

    #[test]
    fn config_info_lines() {
        let t = include_str!("../../tests/data/steam/config_info");
        assert_eq!(
            proton_dir_from_config_info(t),
            Some(PathBuf::from(
                "/home/player/.local/share/Steam/compatibilitytools.d/GE-Proton10-25"
            ))
        );
        assert_eq!(
            proton_dir_from_config_info("9.0\n/s/common/Proton 9.0/dist/share/fonts/\n"),
            Some(PathBuf::from("/s/common/Proton 9.0"))
        );
        for bad in [
            "",
            "x\n",
            "x\nrelative/files/share/fonts/\n",
            "x\n/a/../b/files/share/fonts\n",
        ] {
            assert_eq!(proton_dir_from_config_info(bad), None, "{bad:?}");
        }
    }
}
