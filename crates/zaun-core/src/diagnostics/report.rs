//! The diagnostics report (ERROR_HANDLING §7, PRIVACY §5): a `.tar.gz` the user saves where
//! they want, built in two steps so the user can look at it first.
//!
//! 1. [`build`] collects the files in memory, every one redacted (home → `~`, user name →
//!    `<user>`, host name → `<host>`): `version.txt`, `doctor.json`, `host-facts.json`,
//!    `config.toml` (global environment values replaced by `<set>`), the last 2 000 lines of
//!    the application log and, for one chosen game, its policy and launch configuration
//!    (environment values replaced), and the chosen session's `plan.txt`, `events.jsonl`,
//!    `supervisor.log` and the last 5 000 lines of `game.log`. The UI shows the list with
//!    sizes and a *View* action per file (the preview).
//! 2. [`write_archive`] writes exactly those files as a gzip-compressed POSIX tar archive.
//!
//! Never included: prefix contents, artwork, statistics, other games' names, the full
//! environment, the database file. Only the files named above are ever opened, so nothing
//! else can leak in; the tests plant decoys around them to prove it.

use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::fsx::Fs;
use crate::host::{DoctorReport, HostFacts};
use crate::ids::SessionId;
use crate::logging::Redactor;
use crate::model::{LaunchConfig, SandboxPolicy};
use crate::paths::AppPaths;

/// Lines of the application log included.
pub const APP_LOG_LINES: usize = 2_000;
/// Lines of a session's `game.log` included.
pub const GAME_LOG_LINES: usize = 5_000;
/// What a file is replaced by when it is larger (before taking the tail).
const READ_CAP: u64 = 64 * 1024 * 1024;
/// The replacement of a value that is not included.
pub const SET: &str = "<set>";

/// One file of the report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportFile {
    /// Path inside the archive (`game/plan.txt`).
    pub name: String,
    /// Content (redacted).
    pub bytes: Vec<u8>,
}

impl ReportFile {
    fn text(name: &str, text: String) -> Self {
        Self {
            name: name.to_owned(),
            bytes: text.into_bytes(),
        }
    }
}

/// The optional per-game part.
pub struct GamePart<'a> {
    /// The game's protection settings.
    pub policy: &'a SandboxPolicy,
    /// Its launch configuration (environment values are replaced).
    pub launch: &'a LaunchConfig,
    /// The session whose logs are added.
    pub session: Option<SessionId>,
}

/// Inputs of [`build`].
pub struct ReportInput<'a> {
    /// Filesystem.
    pub fs: &'a dyn Fs,
    /// Locations.
    pub paths: &'a AppPaths,
    /// Redaction.
    pub redactor: &'a Redactor,
    /// `zaun --version` text and anything else about the build.
    pub version: String,
    /// The Doctor report, when one was made.
    pub doctor: Option<&'a DoctorReport>,
    /// Host facts.
    pub facts: Option<&'a HostFacts>,
    /// One game.
    pub game: Option<GamePart<'a>>,
}

fn json<T: Serialize>(v: &T, r: &Redactor) -> String {
    let text = serde_json::to_string_pretty(v).unwrap_or_else(|e| format!("not available: {e}"));
    r.redact_str(&text) + "\n"
}

/// The last `n` lines of a text file, redacted; `None` when it cannot be read.
fn tail(fs: &dyn Fs, path: &Path, n: usize, r: &Redactor) -> Option<String> {
    let md = fs.metadata(path).ok().filter(|m| m.is_file())?;
    let from = md.size.saturating_sub(READ_CAP);
    let bytes = fs
        .read_range(path, from, usize::try_from(md.size - from).ok()?)
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(n);
    let mut out = String::new();
    if start > 0 || from > 0 {
        out.push_str(&format!("[earlier lines not included; last {n} lines]\n"));
    }
    for l in &lines[start..] {
        out.push_str(&r.redact_str(l));
        out.push('\n');
    }
    Some(out)
}

/// `config.toml` with every `launch.global_env` value, `chat.gif_key` and each
/// `chat.ice_servers` entry replaced by `<set>`.
fn config_text(fs: &dyn Fs, path: &Path, r: &Redactor) -> Option<String> {
    let bytes = fs.read(path, 1024 * 1024).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let Ok(mut table) = text.parse::<toml::Table>() else {
        return Some("# config.toml could not be parsed; its content is not included\n".into());
    };
    if let Some(list) = table
        .get_mut("launch")
        .and_then(|l| l.get_mut("global_env"))
        .and_then(|g| g.as_array_mut())
    {
        for v in list.iter_mut() {
            if let Some(t) = v.as_table_mut()
                && t.contains_key("value")
            {
                t.insert("value".into(), toml::Value::String(SET.into()));
            }
        }
    }
    // The GIF search key (ADR-0033) may be the user's own.
    if let Some(chat) = table.get_mut("chat").and_then(|c| c.as_table_mut())
        && chat.contains_key("gif_key")
    {
        chat.insert("gif_key".into(), toml::Value::String(SET.into()));
    }
    // Calls' TURN servers carry passwords (ADR-0035): each entry becomes `<set>`.
    if let Some(list) = table
        .get_mut("chat")
        .and_then(|c| c.get_mut("ice_servers"))
        .and_then(|l| l.as_array_mut())
    {
        for v in list.iter_mut() {
            *v = toml::Value::String(SET.into());
        }
    }
    let out = toml::to_string_pretty(&table).ok()?;
    Some(r.redact_str(&out))
}

/// The launch configuration with every environment value replaced by `<set>`.
fn launch_without_values(launch: &LaunchConfig) -> LaunchConfig {
    let mut l = launch.clone();
    for v in &mut l.env {
        v.value = SET.into();
    }
    l
}

/// Collect the report's files (step 1). Files that do not exist are left out; the order is
/// the order of the preview.
pub fn build(input: &ReportInput<'_>) -> Vec<ReportFile> {
    let r = input.redactor;
    let fs = input.fs;
    let p = input.paths;
    let mut out = vec![ReportFile::text(
        "version.txt",
        r.redact_str(&input.version) + "\n",
    )];
    if let Some(d) = input.doctor {
        out.push(ReportFile::text("doctor.json", json(d, r)));
    }
    if let Some(f) = input.facts.or(input.doctor.map(|d| &d.facts)) {
        out.push(ReportFile::text("host-facts.json", json(f, r)));
    }
    if let Some(c) = config_text(fs, &p.config_file, r) {
        out.push(ReportFile::text("config.toml", c));
    }
    if let Some(t) = tail(fs, &p.app_log_file, APP_LOG_LINES, r) {
        out.push(ReportFile::text("zaun.log", t));
    }
    if let Some(g) = &input.game {
        out.push(ReportFile::text("game/policy.json", json(g.policy, r)));
        out.push(ReportFile::text(
            "game/launch.json",
            json(&launch_without_values(g.launch), r),
        ));
        if let Some(sid) = g.session {
            let dir = p.session_dir(sid);
            for (file, lines) in [
                ("plan.txt", usize::MAX),
                ("events.jsonl", usize::MAX),
                ("supervisor.log", usize::MAX),
                ("game.log", GAME_LOG_LINES),
            ] {
                if let Some(t) = tail(fs, &dir.join(file), lines, r) {
                    out.push(ReportFile::text(&format!("session/{file}"), t));
                }
            }
        }
    }
    out
}

/// Total size of the files.
pub fn total_size(files: &[ReportFile]) -> u64 {
    files.iter().map(|f| f.bytes.len() as u64).sum()
}

// ---- archive --------------------------------------------------------------------------------

fn octal(field: &mut [u8], value: u64) {
    let digits = field.len() - 1;
    let s = format!("{value:0digits$o}");
    let b = s.as_bytes();
    let b = &b[b.len().saturating_sub(digits)..];
    field[..b.len()].copy_from_slice(b);
    field[digits] = 0;
}

fn header(name: &str, size: u64, mtime: u64) -> std::io::Result<[u8; 512]> {
    let mut h = [0u8; 512];
    let n = name.as_bytes();
    if n.len() > 100 || n.is_empty() {
        return Err(std::io::Error::other(format!(
            "archive name too long: {name}"
        )));
    }
    h[..n.len()].copy_from_slice(n);
    octal(&mut h[100..108], 0o600);
    octal(&mut h[108..116], 0);
    octal(&mut h[116..124], 0);
    octal(&mut h[124..136], size);
    octal(&mut h[136..148], mtime);
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    // The checksum is computed with its own field as spaces.
    h[148..156].copy_from_slice(b"        ");
    let sum: u64 = h.iter().map(|b| u64::from(*b)).sum();
    let s = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(s.as_bytes());
    Ok(h)
}

/// Write `files` as a `.tar.gz` (step 2). `mtime` is Unix seconds for every entry.
pub fn write_archive(files: &[ReportFile], mtime: u64, out: impl Write) -> std::io::Result<()> {
    let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::default());
    for f in files {
        let name = format!("zaun-report/{}", f.name);
        gz.write_all(&header(&name, f.bytes.len() as u64, mtime)?)?;
        gz.write_all(&f.bytes)?;
        let pad = (512 - f.bytes.len() % 512) % 512;
        gz.write_all(&vec![0u8; pad])?;
    }
    gz.write_all(&[0u8; 1024])?;
    gz.finish()?.flush()
}

/// The archive as bytes.
pub fn archive_bytes(files: &[ReportFile], mtime: u64) -> std::io::Result<Vec<u8>> {
    let mut v = Vec::new();
    write_archive(files, mtime, &mut v)?;
    Ok(v)
}

/// Read back an archive written by [`write_archive`] (tests, and the CLI's check of what was
/// saved): `(name, bytes)` per entry.
pub fn read_archive(bytes: &[u8]) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    use std::io::Read;
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(bytes).read_to_end(&mut raw)?;
    let mut out = Vec::new();
    let mut at = 0;
    while at + 512 <= raw.len() {
        let h = &raw[at..at + 512];
        if h.iter().all(|b| *b == 0) {
            break;
        }
        let name_end = h[..100].iter().position(|b| *b == 0).unwrap_or(100);
        let name = String::from_utf8_lossy(&h[..name_end]).into_owned();
        let size_text = String::from_utf8_lossy(&h[124..135])
            .trim_matches('\0')
            .trim()
            .to_owned();
        let size = usize::from_str_radix(&size_text, 8).map_err(std::io::Error::other)?;
        at += 512;
        let end = at + size;
        if end > raw.len() {
            return Err(std::io::Error::other("truncated archive"));
        }
        out.push((name, raw[at..end].to_vec()));
        at = end + (512 - size % 512) % 512;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsx::RealFs;
    use crate::model::EnvVar;
    use crate::paths::PathEnv;

    #[test]
    fn archive_round_trips_and_tar_reads_it() {
        let files = vec![
            ReportFile::text("a.txt", "hello\n".into()),
            ReportFile {
                name: "dir/b.bin".into(),
                bytes: vec![0u8; 700],
            },
        ];
        let bytes = archive_bytes(&files, 1_700_000_000).unwrap();
        let back = read_archive(&bytes).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].0, "zaun-report/a.txt");
        assert_eq!(back[0].1, b"hello\n");
        assert_eq!(back[1].1.len(), 700);
        assert!(header(&"x".repeat(101), 0, 0).is_err());
        // GNU/BSD tar agree, when one is installed.
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("r.tar.gz");
        RealFs::new().atomic_write(&f, &bytes, 0o600).unwrap();
        #[allow(clippy::disallowed_methods)]
        if let Ok(out) = std::process::Command::new("tar")
            .arg("-tzf")
            .arg(&f)
            .output()
        {
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(
                String::from_utf8_lossy(&out.stdout),
                "zaun-report/a.txt\nzaun-report/dir/b.bin\n"
            );
        }
    }

    /// ERR-003 acceptance: a report built from a fixture with decoy secrets in excluded
    /// locations contains none of them.
    #[test]
    fn report_contains_no_decoys() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home/sam");
        let fs = RealFs::new();
        let env = PathEnv::new(1000)
            .with("HOME", home.as_os_str())
            .with("XDG_RUNTIME_DIR", dir.path().join("run").as_os_str());
        let paths = AppPaths::resolve(&env).unwrap();
        let w = |p: &Path, t: &str| {
            fs.create_dir_all(p.parent().unwrap(), 0o700).unwrap();
            fs.atomic_write(p, t.as_bytes(), 0o600).unwrap();
        };
        let game = crate::ids::GameId::from_uuid(uuid::Uuid::from_u128(7));
        let other = crate::ids::GameId::from_uuid(uuid::Uuid::from_u128(8));
        let sid = SessionId::from_uuid(uuid::Uuid::from_u128(9));
        // Included sources, with personal data that must be redacted.
        w(
            &paths.config_file,
            "[launch]\nglobal_env = [{ name = \"API_TOKEN\", value = \"DECOY-GLOBAL-VALUE\" }]\n[paths]\numu_run = \"/home/sam/bin/umu-run\"\n[chat]\ngif_key = \"DECOY-GIF-KEY\"\nice_servers = [\"turn:u:DECOY-TURN-PASSWORD@relay.example\"]\n",
        );
        let home_s = home.display().to_string();
        let mut log = String::new();
        for i in 0..3_000 {
            log.push_str(&format!("line {i} opened {home_s}/Games\n"));
        }
        w(&paths.app_log_file, &log);
        let sdir = paths.session_dir(sid);
        w(&sdir.join("plan.txt"), &format!("bwrap {home_s}/x\n"));
        w(&sdir.join("events.jsonl"), "{\"t\":1}\n");
        w(&sdir.join("game.log"), "[+0.1] hello\n");
        // Decoys in places that are never included.
        w(
            &paths.prefix_dir(game).join("drive_c/secret.txt"),
            "DECOY-PREFIX",
        );
        w(&paths.db_file, "DECOY-DATABASE");
        w(&paths.artwork_game_dir(game).join("cover.png"), "DECOY-ART");
        w(
            &paths
                .session_dir(SessionId::from_uuid(uuid::Uuid::from_u128(10)))
                .join("game.log"),
            "DECOY-OTHER-SESSION",
        );
        w(&paths.game_dir(other).join("name.txt"), "DECOY-OTHER-GAME");
        w(&home.join(".ssh/id_ed25519"), "DECOY-KEY");

        let launch = LaunchConfig {
            env: vec![EnvVar {
                name: "MY_SECRET".into(),
                value: "DECOY-GAME-VALUE".into(),
            }],
            ..LaunchConfig::default()
        };
        let policy = crate::sandbox::offline();
        let r = Redactor::new(Some(&home), Some("sam"), Some("gamebox"));
        let files = build(&ReportInput {
            fs: &fs,
            paths: &paths,
            redactor: &r,
            version: "zaun 0.1.0 (abc) on gamebox".into(),
            doctor: None,
            facts: None,
            game: Some(GamePart {
                policy: &policy,
                launch: &launch,
                session: Some(sid),
            }),
        });
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "version.txt",
                "config.toml",
                "zaun.log",
                "game/policy.json",
                "game/launch.json",
                "session/plan.txt",
                "session/events.jsonl",
                "session/game.log"
            ]
        );
        let archive = archive_bytes(&files, 0).unwrap();
        let entries = read_archive(&archive).unwrap();
        let all: String = entries
            .iter()
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
            .collect();
        for decoy in [
            "DECOY-GLOBAL-VALUE",
            "DECOY-GAME-VALUE",
            "DECOY-PREFIX",
            "DECOY-DATABASE",
            "DECOY-ART",
            "DECOY-OTHER-SESSION",
            "DECOY-OTHER-GAME",
            "DECOY-KEY",
            "DECOY-GIF-KEY",
            "DECOY-TURN-PASSWORD",
            &home_s,
            "gamebox",
        ] {
            assert!(!all.contains(decoy), "{decoy} leaked");
        }
        assert!(all.contains("API_TOKEN") && all.contains(SET));
        assert!(all.contains("MY_SECRET"));
        let log = &files[2];
        let text = String::from_utf8_lossy(&log.bytes);
        assert_eq!(text.lines().count(), APP_LOG_LINES + 1);
        assert!(text.contains("line 2999 opened ~/Games"));
        assert!(!text.contains("line 999 "));
    }
}
