//! One read-only pass over every Steam installation (ADR-0031 §2–§4).
//!
//! For each root ([`super::roots::find_roots`]): its library folders, every
//! `appmanifest_<appid>.acf` in them, the tool mapping of `config/config.vdf`, the play times
//! of the most recently used account, each game's compatibility data and cached pictures.
//! Every read goes through [`Fs`] with a size limit; nothing is written. An app listed by two
//! installations (native and Flatpak) is taken from the first.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use super::account::{AppPlay, read_play_times};
use super::art::{CachedArt, read_art};
use super::compat::{CONFIG_MAX, read_compat, tool_mapping};
use super::manifest::{
    AppManifest, MANIFEST_MAX, NotAGame, classify, install_path, manifest_app_id, parse_manifest,
};
use super::roots::{SteamRoot, find_roots, library_folders};
use crate::clock::Timestamp;
use crate::fsx::{FileType, Fs};
use crate::model::SteamFacts;

/// Most manifests read per library folder.
pub const MAX_MANIFESTS_PER_FOLDER: usize = 20_000;

/// An installed Steam game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamGame {
    /// Display name from the manifest.
    pub name: String,
    /// Facts shown in the library.
    pub facts: SteamFacts,
    /// Last played (Steam's account record, else the manifest).
    pub last_played: Option<Timestamp>,
    /// Pictures from Steam's cache.
    pub art: Vec<CachedArt>,
    /// The installation's root.
    pub root: PathBuf,
    /// The install folder exists (its drive is connected).
    pub present: bool,
}

/// An installed app that is not a game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// App ID.
    pub app_id: u32,
    /// Name.
    pub name: String,
    /// Why.
    pub reason: NotAGame,
}

/// The result of a scan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SteamScan {
    /// The installations found.
    pub roots: Vec<SteamRoot>,
    /// Games, by app ID.
    pub games: Vec<SteamGame>,
    /// Apps left out, by app ID.
    pub skipped: Vec<Skipped>,
}

impl SteamScan {
    /// A game by app ID.
    pub fn game(&self, app_id: u32) -> Option<&SteamGame> {
        self.games.iter().find(|g| g.facts.app_id == app_id)
    }
}

/// The manifests of one library folder.
fn manifests(fs: &dyn Fs, folder: &Path) -> Vec<AppManifest> {
    let Ok(entries) = fs.read_dir(&folder.join("steamapps")) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter(|e| e.file_type == FileType::File)
        .filter_map(|e| Some((manifest_app_id(e.name.to_str()?)?, e.path)))
        .take(MAX_MANIFESTS_PER_FOLDER)
        .filter_map(|(id, path)| {
            let m = parse_manifest(&fs.read(&path, MANIFEST_MAX).ok()?)?;
            // A manifest whose app ID disagrees with its file name is not trusted.
            (m.app_id == id).then_some(m)
        })
        .collect()
}

fn game_of(
    fs: &dyn Fs,
    root: &SteamRoot,
    folder: &Path,
    m: AppManifest,
    plays: &HashMap<u32, AppPlay>,
    mapping: &HashMap<u32, String>,
) -> SteamGame {
    let install_dir = install_path(folder, &m);
    let present = fs.metadata_follow(&install_dir).is_ok_and(|md| md.is_dir());
    let play = plays.get(&m.app_id).copied();
    let facts = SteamFacts {
        app_id: m.app_id,
        install: root.kind,
        library_folder: folder.to_path_buf(),
        size_on_disk: m.size_on_disk,
        build_id: m.build_id,
        last_updated: m.last_updated,
        update: m.update(),
        download: m.download(),
        files_missing: m.files_missing(),
        compat: read_compat(fs, m.app_id, folder, &root.path, mapping),
        playtime_minutes: play.map(|p| p.minutes),
        install_dir,
    };
    SteamGame {
        last_played: play.and_then(|p| p.last_played).or(m.last_played),
        art: read_art(fs, &root.path, m.app_id),
        root: root.path.clone(),
        present,
        name: m.name,
        facts,
    }
}

/// Scan every Steam installation under `home`.
pub fn scan(fs: &dyn Fs, home: &Path) -> SteamScan {
    let roots = find_roots(fs, home);
    let mut games: BTreeMap<u32, SteamGame> = BTreeMap::new();
    let mut skipped: BTreeMap<u32, Skipped> = BTreeMap::new();
    for root in &roots {
        let mapping = fs
            .read(&root.path.join("config/config.vdf"), CONFIG_MAX)
            .map(|b| tool_mapping(&b))
            .unwrap_or_default();
        let plays = read_play_times(fs, &root.path);
        for folder in library_folders(fs, &root.path) {
            for m in manifests(fs, &folder) {
                if !m.is_installed()
                    || games.contains_key(&m.app_id)
                    || skipped.contains_key(&m.app_id)
                {
                    continue;
                }
                let tool_manifest = fs.exists(&install_path(&folder, &m).join("toolmanifest.vdf"));
                if let Some(reason) = classify(m.app_id, &m.name, tool_manifest) {
                    skipped.insert(
                        m.app_id,
                        Skipped {
                            app_id: m.app_id,
                            name: m.name,
                            reason,
                        },
                    );
                    continue;
                }
                let g = game_of(fs, root, &folder, m, &plays, &mapping);
                games.insert(g.facts.app_id, g);
            }
        }
    }
    SteamScan {
        roots,
        games: games.into_values().collect(),
        skipped: skipped.into_values().collect(),
    }
}
