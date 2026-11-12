//! Exported game settings (CONFIGURATION §4): everything needed to recreate a game's
//! configuration, but not the prefix, saves, artwork or statistics.
//!
//! Import is validated like user input: [`import`] turns a file into [`ImportedSettings`]
//! for this host (unknown keys rejected, every value through the model validators), and the
//! result is applied through the same paths as manual edits ([`ImportedSettings::apply_to_draft`]
//! for setup, [`Library::import_settings`] → `Library::update` for an existing game).
//! Absolute folder paths (`UserChosen` grants, an external save folder, an external
//! compatibility layer) are listed for the user to re-grant, never applied.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::draft::GameDraft;
use super::service::{Library, PatchWarning, storage};
use crate::clock::Timestamp;
use crate::error::{Error, ErrorCode, Result};
use crate::ids::{GameId, RuntimeName, TagName};
use crate::model::{
    AudioAccess, BasePreset, Channel, CompatOptions, ControllerAccess, EnvVar, Fingerprint,
    FolderAccess, FolderGrant, Game, GameDirAccess, GrantOrigin, NetworkAccess, Preset,
    RemovableAccess, RuntimeRef, SandboxPolicy, SaveDataLocation, Store, UserDir, WorkingDir,
    to_hex,
};
use crate::storage::{GamePatch, TagRepo};

/// The format version written and read.
pub const EXPORT_VERSION: u32 = 1;
/// Largest file accepted.
pub const EXPORT_MAX_BYTES: usize = 256 * 1024;

/// The file (CONFIGURATION §4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportFile {
    /// Format version.
    pub easyplay_export: u32,
    /// RFC 3339.
    pub exported_at: String,
    /// Identity.
    pub game: ExGame,
    /// The primary program.
    pub program: ExProgram,
    /// Compatibility layer.
    pub compatibility: ExCompat,
    /// Protection.
    pub protection: ExProtection,
    /// Environment variables.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment: Vec<ExEnv>,
}

/// `[game]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExGame {
    /// Name.
    pub name: String,
    /// umu `GAMEID`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub umu_id: Option<String>,
    /// umu `STORE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    /// Tag names.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// `[program]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExProgram {
    /// File name of the primary executable.
    pub file_name: String,
    /// `sha256:<hex>` quick hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_hash: Option<String>,
    /// Path relative to the game folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative_to_game_folder: Option<String>,
    /// Arguments.
    #[serde(default)]
    pub arguments: Vec<String>,
    /// `exe-dir`, `game-folder` or `custom`.
    pub working_dir: String,
    /// For `custom`: the directory relative to the game folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir_relative: Option<String>,
}

/// `[compatibility]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExCompat {
    /// `default`, `channel:umu-proton`, `channel:ge-proton`, `pinned:<name>`, `external:<path>`.
    pub layer: String,
    /// Follow updates.
    pub follow_updates: bool,
    /// Compatibility options (missing keys take their defaults).
    #[serde(default)]
    pub options: Option<toml::Table>,
}

/// `[[protection.folders]]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExFolder {
    /// A well-known directory (`music`, `documents`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub well_known: Option<String>,
    /// A user-chosen absolute path (listed on import, not applied).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `read-only` or `read-write`.
    pub access: String,
    /// Drive letter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_letter: Option<String>,
}

/// `[protection.devices]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExDevices {
    /// `full` or `none`.
    pub audio: String,
    /// `none`, `detected` or `all-input`.
    pub controllers: String,
    /// Cameras.
    #[serde(default)]
    pub camera: bool,
}

/// `[protection]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExProtection {
    /// `offline`, `online`, `standard`, `custom:<base>`.
    pub preset: String,
    /// `none` or `full`.
    pub network: String,
    /// `file-only`, `read-only`, `read-write`, `protected`.
    pub game_dir: String,
    /// `internal` or `external`.
    pub save_data: String,
    /// The external save folder (listed on import, not applied).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_dir: Option<String>,
    /// `none`, `read-only`, `read-write`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removable: Option<String>,
    /// Folder grants.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<ExFolder>,
    /// Devices.
    pub devices: ExDevices,
}

/// `[[environment]]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExEnv {
    /// Name.
    pub name: String,
    /// Value.
    pub value: String,
}

fn user_dir_name(d: UserDir) -> &'static str {
    match d {
        UserDir::Desktop => "desktop",
        UserDir::Documents => "documents",
        UserDir::Downloads => "downloads",
        UserDir::Music => "music",
        UserDir::Pictures => "pictures",
        UserDir::Videos => "videos",
    }
}

fn parse_user_dir(s: &str) -> Option<UserDir> {
    [
        UserDir::Desktop,
        UserDir::Documents,
        UserDir::Downloads,
        UserDir::Music,
        UserDir::Pictures,
        UserDir::Videos,
    ]
    .into_iter()
    .find(|d| user_dir_name(*d) == s)
}

fn layer_text(r: &RuntimeRef) -> String {
    match r {
        RuntimeRef::Default => "default".into(),
        RuntimeRef::Channel(Channel::UmuProton) => "channel:umu-proton".into(),
        RuntimeRef::Channel(Channel::GeProton) => "channel:ge-proton".into(),
        RuntimeRef::Pinned(n) => format!("pinned:{}", n.as_str()),
        RuntimeRef::External(p) => format!("external:{}", p.display()),
    }
}

fn game_dir_text(g: GameDirAccess) -> &'static str {
    match g {
        GameDirAccess::FileOnly => "file-only",
        GameDirAccess::ReadOnly => "read-only",
        GameDirAccess::ReadWrite => "read-write",
        GameDirAccess::Protected => "protected",
    }
}

/// Write the export of `game` with its tag names.
pub fn export(game: &Game, tags: &[TagName], now: Timestamp) -> Result<String> {
    let primary = game.primary().ok_or_else(|| super::no_game(game.id))?;
    let rel = |p: &Path| -> Option<String> {
        game.root
            .as_deref()
            .and_then(|r| p.strip_prefix(r).ok())
            .map(|r| r.to_string_lossy().into_owned())
    };
    let (working_dir, working_dir_relative) = match &game.launch.working_dir {
        WorkingDir::ExeDir => ("exe-dir", None),
        WorkingDir::GameRoot => ("game-folder", None),
        WorkingDir::Custom(p) => ("custom", rel(p)),
    };
    let policy = &game.policy;
    let options = toml::Table::try_from(&game.launch.compat)
        .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("reason", e.to_string()))?;
    let file = ExportFile {
        easyplay_export: EXPORT_VERSION,
        exported_at: now.to_rfc3339(),
        game: ExGame {
            name: game.name.clone(),
            umu_id: game.umu_id.clone(),
            store: game.store.as_ref().map(|s| s.as_str().to_owned()),
            tags: tags.iter().map(|t| t.as_str().to_owned()).collect(),
        },
        program: ExProgram {
            file_name: primary
                .canonical_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            quick_hash: Some(format!(
                "sha256:{}",
                to_hex(&primary.fingerprint.quick_hash)
            )),
            relative_to_game_folder: rel(&primary.canonical_path),
            arguments: game.launch.args.clone(),
            working_dir: working_dir.into(),
            working_dir_relative,
        },
        compatibility: ExCompat {
            layer: layer_text(&game.runtime),
            follow_updates: game.runtime_follow_updates,
            options: Some(options),
        },
        protection: ExProtection {
            preset: policy.preset.as_column(),
            network: policy.network.as_str().into(),
            game_dir: game_dir_text(policy.game_dir).into(),
            save_data: match &policy.save_data {
                SaveDataLocation::Internal => "internal".into(),
                SaveDataLocation::External { .. } => "external".into(),
            },
            save_dir: match &policy.save_data {
                SaveDataLocation::External { dir } => Some(dir.display().to_string()),
                SaveDataLocation::Internal => None,
            },
            removable: Some(
                match policy.removable {
                    RemovableAccess::None => "none",
                    RemovableAccess::ReadOnly => "read-only",
                    RemovableAccess::ReadWrite => "read-write",
                }
                .into(),
            ),
            folders: policy
                .folders
                .iter()
                .map(|f| ExFolder {
                    well_known: match f.origin {
                        GrantOrigin::WellKnown(d) => Some(user_dir_name(d).into()),
                        GrantOrigin::UserChosen => None,
                    },
                    path: match f.origin {
                        GrantOrigin::UserChosen => Some(f.path.display().to_string()),
                        GrantOrigin::WellKnown(_) => None,
                    },
                    access: match f.access {
                        FolderAccess::ReadOnly => "read-only".into(),
                        FolderAccess::ReadWrite => "read-write".into(),
                    },
                    drive_letter: f.drive_letter.map(|c| c.to_string()),
                })
                .collect(),
            devices: ExDevices {
                audio: match policy.devices.audio {
                    AudioAccess::Full => "full".into(),
                    AudioAccess::None => "none".into(),
                },
                controllers: match policy.devices.controllers {
                    ControllerAccess::None => "none".into(),
                    ControllerAccess::Detected => "detected".into(),
                    ControllerAccess::AllInput => "all-input".into(),
                },
                camera: policy.devices.camera,
            },
        },
        environment: game
            .launch
            .env
            .iter()
            .map(|v| ExEnv {
                name: v.name.clone(),
                value: v.value.clone(),
            })
            .collect(),
    };
    toml::to_string(&file)
        .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("reason", e.to_string()))
}

/// Something the user must look at after an import.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportNote {
    /// The chosen executable's quick hash differs from the exported one (a warning).
    QuickHashMismatch,
    /// A user-chosen folder to re-grant (not applied).
    Regrant {
        /// The exported path.
        path: PathBuf,
        /// Access.
        access: FolderAccess,
        /// Drive letter.
        drive_letter: Option<char>,
    },
    /// An external save folder (not applied; saves stay internal).
    ExternalSaveDir(PathBuf),
    /// An external compatibility layer (not applied; the default is used).
    ExternalLayer(PathBuf),
    /// A well-known folder that does not exist on this host.
    MissingUserDir(UserDir),
}

/// Host facts the import needs.
pub struct ImportContext<'a> {
    /// The preset policy for a base (`sandbox::presets::preset_policy` with this host).
    pub preset_policy: &'a dyn Fn(BasePreset) -> SandboxPolicy,
    /// XDG user directories.
    pub user_dirs: &'a [(UserDir, PathBuf)],
    /// The executable the settings are imported for.
    pub exe_fingerprint: Option<&'a Fingerprint>,
    /// Whether that game has a folder (otherwise `file-only` is forced).
    pub has_root: bool,
    /// The game folder, for relative working directories.
    pub root: Option<&'a Path>,
    /// Grant timestamps.
    pub now: Timestamp,
}

/// Settings ready to apply.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedSettings {
    /// Name.
    pub name: String,
    /// umu `GAMEID`.
    pub umu_id: Option<String>,
    /// umu `STORE`.
    pub store: Option<Store>,
    /// Tags.
    pub tags: Vec<TagName>,
    /// Arguments.
    pub args: Vec<String>,
    /// Working directory.
    pub working_dir: WorkingDir,
    /// Environment.
    pub env: Vec<EnvVar>,
    /// Compatibility layer.
    pub runtime: RuntimeRef,
    /// Follow updates.
    pub follow_updates: bool,
    /// Options.
    pub compat: CompatOptions,
    /// Policy (grants of well-known folders applied; absolute ones listed).
    pub policy: SandboxPolicy,
    /// Things to show the user.
    pub notes: Vec<ImportNote>,
}

fn bad(field: &'static str) -> Error {
    Error::new(ErrorCode::CfgInvalidValue).with("field", field)
}

/// Parse an export file (size-capped, unknown keys rejected, version checked).
pub fn parse(text: &str) -> Result<ExportFile> {
    if text.len() > EXPORT_MAX_BYTES {
        return Err(bad("file").with("reason", "too large"));
    }
    let file: ExportFile = toml::from_str(text).map_err(|e| {
        Error::new(ErrorCode::CfgInvalidValue).with("reason", e.message().to_owned())
    })?;
    if file.easyplay_export != EXPORT_VERSION {
        return Err(Error::new(ErrorCode::CfgNewerVersion).with("version", file.easyplay_export));
    }
    Ok(file)
}

fn parse_access(s: &str) -> Result<FolderAccess> {
    match s {
        "read-only" => Ok(FolderAccess::ReadOnly),
        "read-write" => Ok(FolderAccess::ReadWrite),
        _ => Err(bad("protection.folders.access")),
    }
}

fn parse_letter(s: Option<&str>) -> Result<Option<char>> {
    match s {
        None => Ok(None),
        Some(s) => {
            let mut c = s.chars();
            match (c.next(), c.next()) {
                (Some(l), None) if crate::model::is_valid_drive_letter(l.to_ascii_lowercase()) => {
                    Ok(Some(l.to_ascii_lowercase()))
                }
                _ => Err(bad("protection.folders.drive_letter")),
            }
        }
    }
}

/// Turn a parsed file into settings for this host.
pub fn import(file: &ExportFile, ctx: &ImportContext<'_>) -> Result<ImportedSettings> {
    let mut notes = Vec::new();
    crate::model::validate_name(&file.game.name)?;
    if let Some(id) = &file.game.umu_id {
        crate::model::validate_umu_id(id)?;
    }
    let tags = file
        .game
        .tags
        .iter()
        .take(super::service::MAX_TAGS_PER_GAME + 1)
        .map(|t| TagName::new(t).map_err(|_| bad("game.tags")))
        .collect::<Result<Vec<_>>>()?;
    if tags.len() > super::service::MAX_TAGS_PER_GAME {
        return Err(super::limit("game.tags", super::service::MAX_TAGS_PER_GAME));
    }
    if let (Some(fp), Some(h)) = (ctx.exe_fingerprint, &file.program.quick_hash)
        && h.strip_prefix("sha256:") != Some(to_hex(&fp.quick_hash).as_str())
    {
        notes.push(ImportNote::QuickHashMismatch);
    }
    crate::model::validate_args("program.arguments", &file.program.arguments)?;
    let working_dir = match file.program.working_dir.as_str() {
        "exe-dir" => WorkingDir::ExeDir,
        "game-folder" if ctx.has_root => WorkingDir::GameRoot,
        "game-folder" => WorkingDir::ExeDir,
        "custom" => match (ctx.root, &file.program.working_dir_relative) {
            (Some(root), Some(rel)) => {
                let rel = Path::new(rel);
                if rel.is_absolute()
                    || rel
                        .components()
                        .any(|c| !matches!(c, std::path::Component::Normal(_)))
                {
                    return Err(bad("program.working_dir_relative"));
                }
                WorkingDir::Custom(root.join(rel))
            }
            _ => WorkingDir::ExeDir,
        },
        _ => return Err(bad("program.working_dir")),
    };
    let (runtime, layer_note) = match file.compatibility.layer.as_str() {
        "default" => (RuntimeRef::Default, None),
        "channel:umu-proton" => (RuntimeRef::Channel(Channel::UmuProton), None),
        "channel:ge-proton" => (RuntimeRef::Channel(Channel::GeProton), None),
        s => {
            if let Some(n) = s.strip_prefix("pinned:") {
                (
                    RuntimeRef::Pinned(
                        RuntimeName::new(n).map_err(|_| bad("compatibility.layer"))?,
                    ),
                    None,
                )
            } else if let Some(p) = s.strip_prefix("external:") {
                (
                    RuntimeRef::Default,
                    Some(ImportNote::ExternalLayer(PathBuf::from(p))),
                )
            } else {
                return Err(bad("compatibility.layer"));
            }
        }
    };
    notes.extend(layer_note);
    let compat = match &file.compatibility.options {
        None => CompatOptions::default(),
        Some(t) => {
            let mut base = toml::Table::try_from(CompatOptions::default())
                .map_err(|_| bad("compatibility.options"))?;
            for (k, v) in t {
                if !base.contains_key(k) {
                    return Err(bad("compatibility.options"));
                }
                base.insert(k.clone(), v.clone());
            }
            let c: CompatOptions = base.try_into().map_err(|_| bad("compatibility.options"))?;
            c.validate()?;
            c
        }
    };
    let p = &file.protection;
    let preset = Preset::from_column(&p.preset).ok_or_else(|| bad("protection.preset"))?;
    let mut policy = (ctx.preset_policy)(preset.base());
    policy.network = match p.network.as_str() {
        "none" => NetworkAccess::None,
        "full" => NetworkAccess::Full,
        _ => return Err(bad("protection.network")),
    };
    policy.game_dir = match (p.game_dir.as_str(), ctx.has_root) {
        (_, false) => GameDirAccess::FileOnly,
        ("file-only" | "read-write", true) => GameDirAccess::ReadWrite,
        ("read-only", true) => GameDirAccess::ReadOnly,
        ("protected", true) => GameDirAccess::Protected,
        _ => return Err(bad("protection.game_dir")),
    };
    match p.save_data.as_str() {
        "internal" => {}
        "external" => notes.extend(
            p.save_dir
                .as_ref()
                .map(|d| ImportNote::ExternalSaveDir(PathBuf::from(d))),
        ),
        _ => return Err(bad("protection.save_data")),
    }
    policy.save_data = SaveDataLocation::Internal;
    if let Some(r) = &p.removable {
        policy.removable = match r.as_str() {
            "none" => RemovableAccess::None,
            "read-only" => RemovableAccess::ReadOnly,
            "read-write" => RemovableAccess::ReadWrite,
            _ => return Err(bad("protection.removable")),
        };
    }
    policy.folders.clear();
    for f in &p.folders {
        let access = parse_access(&f.access)?;
        let drive_letter = parse_letter(f.drive_letter.as_deref())?;
        match (&f.well_known, &f.path) {
            (Some(w), None) => {
                let d = parse_user_dir(w).ok_or_else(|| bad("protection.folders.well_known"))?;
                match ctx.user_dirs.iter().find(|(k, _)| *k == d) {
                    Some((_, path)) => policy.folders.push(FolderGrant {
                        path: path.clone(),
                        canonical: path.clone(),
                        access,
                        origin: GrantOrigin::WellKnown(d),
                        drive_letter,
                        granted_at: ctx.now,
                    }),
                    None => notes.push(ImportNote::MissingUserDir(d)),
                }
            }
            (None, Some(path)) => notes.push(ImportNote::Regrant {
                path: PathBuf::from(path),
                access,
                drive_letter,
            }),
            _ => return Err(bad("protection.folders")),
        }
    }
    policy.devices.audio = match p.devices.audio.as_str() {
        "full" => AudioAccess::Full,
        "none" => AudioAccess::None,
        _ => return Err(bad("protection.devices.audio")),
    };
    policy.devices.controllers = match p.devices.controllers.as_str() {
        "none" => ControllerAccess::None,
        "detected" => ControllerAccess::Detected,
        "all-input" => ControllerAccess::AllInput,
        _ => return Err(bad("protection.devices.controllers")),
    };
    policy.devices.camera = p.devices.camera;
    policy.preset = crate::sandbox::classify(&policy);
    // Keep the exported base when the result is custom.
    if let (Preset::Custom { .. }, Preset::Custom { base }) = (policy.preset, preset) {
        policy.preset = Preset::Custom { base };
    }
    policy.validate()?;
    let env: Vec<EnvVar> = file
        .environment
        .iter()
        .map(|e| EnvVar {
            name: e.name.clone(),
            value: e.value.clone(),
        })
        .collect();
    for v in &env {
        v.validate()?;
    }
    Ok(ImportedSettings {
        name: file.game.name.trim().to_owned(),
        umu_id: file.game.umu_id.clone(),
        store: file.game.store.as_deref().map(Store::parse),
        tags,
        args: file.program.arguments.clone(),
        working_dir,
        env,
        runtime,
        follow_updates: file.compatibility.follow_updates,
        compat,
        policy,
        notes,
    })
}

impl ImportedSettings {
    /// Apply to a setup draft ("Import settings…" in setup); the dialog re-validates.
    pub fn apply_to_draft(&self, draft: &mut GameDraft) {
        draft.set_name(self.name.clone());
        draft.umu_id = self.umu_id.clone();
        draft.tags = self.tags.clone();
        draft.launch.args = self.args.clone();
        draft.launch.working_dir = self.working_dir.clone();
        draft.launch.env = self.env.clone();
        draft.launch.compat = self.compat.clone();
        draft.runtime = self.runtime.clone();
        let game_dir = if draft.root.is_some() {
            if self.policy.game_dir == GameDirAccess::FileOnly {
                GameDirAccess::ReadWrite
            } else {
                self.policy.game_dir
            }
        } else {
            GameDirAccess::FileOnly
        };
        draft.policy = SandboxPolicy {
            game_dir,
            ..self.policy.clone()
        };
    }

    /// The patch that applies these settings to `game`.
    pub fn to_patch(&self, game: &Game) -> GamePatch {
        let mut launch = game.launch.clone();
        launch.args = self.args.clone();
        launch.working_dir = self.working_dir.clone();
        launch.env = self.env.clone();
        launch.compat = self.compat.clone();
        let mut policy = self.policy.clone();
        if game.root.is_none() {
            policy.game_dir = game.policy.game_dir;
        }
        GamePatch {
            name: Some(self.name.clone()),
            name_is_user_set: Some(true),
            umu_id: Some(self.umu_id.clone()),
            store: Some(self.store.clone()),
            runtime: Some(self.runtime.clone()),
            runtime_follow_updates: Some(self.follow_updates),
            policy: Some(policy),
            launch: Some(launch),
            ..GamePatch::default()
        }
    }
}

impl Library {
    /// `easyplay export <game>`.
    pub fn export_game(&mut self, id: GameId) -> Result<String> {
        let game = self.game(id)?;
        let tag_ids = self.db.tags_of(id).map_err(storage)?;
        let all = self.db.tags_with_counts().map_err(storage)?;
        let tags: Vec<TagName> = all
            .into_iter()
            .filter(|(t, _)| tag_ids.contains(&t.id))
            .map(|(t, _)| t.name)
            .collect();
        export(&game, &tags, self.deps.clock.now())
    }

    /// Import settings into an existing game: validated exactly like a manual edit
    /// (`Library::update`), tags through `Library::set_tags`.
    pub fn import_settings(
        &mut self,
        id: GameId,
        text: &str,
        preset_policy: &dyn Fn(BasePreset) -> SandboxPolicy,
        user_dirs: &[(UserDir, PathBuf)],
    ) -> Result<(ImportedSettings, Vec<PatchWarning>)> {
        let game = self.game(id)?;
        let file = parse(text)?;
        let settings = import(
            &file,
            &ImportContext {
                preset_policy,
                user_dirs,
                exe_fingerprint: game.primary().map(|e| &e.fingerprint),
                has_root: game.root.is_some(),
                root: game.root.as_deref(),
                now: self.deps.clock.now(),
            },
        )?;
        let warnings = self.update(id, settings.to_patch(&game))?;
        self.set_tags(id, &settings.tags)?;
        Ok((settings, warnings))
    }
}
