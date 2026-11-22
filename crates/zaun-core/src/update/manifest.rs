//! The release manifest: its signature, its shape, and the bundle's files (ADR-0040 §2,
//! ADR-0041 §3).

use base64::Engine as _;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::Deserialize;

use super::UpdateError;
use super::source::{
    BUNDLE_CHANNEL, BUNDLE_FILES, DOWNLOAD_PREFIX, MAX_FILE, MAX_FOLDER, MAX_MANIFEST, NOTES_PREFIX,
};
use super::version::Version;

/// The longest notes text kept.
const MAX_NOTES: usize = 280;

#[derive(Deserialize)]
struct Raw {
    format: u32,
    product: String,
    version: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    notes_url: Option<String>,
    #[serde(default)]
    artifacts: Vec<Artifact>,
}

/// One file or folder of a release, for one channel and architecture.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Artifact {
    pub channel: String,
    pub arch: String,
    /// Where it goes in a version folder (one of `BUNDLE_FILES`; a folder ends in `/`).
    pub path: String,
    pub url: String,
    /// Bytes downloaded (gzip).
    pub size: u64,
    pub sha256: String,
    /// Bytes once unpacked (a folder: of its tar stream).
    pub unpacked_size: u64,
    pub unpacked_sha256: String,
}

impl Artifact {
    /// A folder, shipped as a tar of plain files.
    pub fn is_folder(&self) -> bool {
        self.path.ends_with('/')
    }

    /// The name in a version folder (`simplex/` → `simplex`).
    pub fn name(&self) -> &str {
        self.path.trim_end_matches('/')
    }
}

/// A manifest whose signature verified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub date: String,
    pub notes: String,
    pub notes_url: Option<String>,
    pub artifacts: Vec<Artifact>,
    /// The signed bytes and the signature text, kept with a staged update.
    pub manifest: Vec<u8>,
    pub signature: Vec<u8>,
}

/// Verify `signature` (base64) over the exact bytes of `manifest` with any of `keys`, then
/// read it. Nothing of an unverified manifest is parsed.
pub fn verify(
    manifest: &[u8],
    signature: &[u8],
    keys: &[[u8; 32]],
) -> Result<Release, UpdateError> {
    if keys.is_empty() {
        return Err(UpdateError::NotConfigured);
    }
    if manifest.len() as u64 > MAX_MANIFEST {
        return Err(UpdateError::BadManifest("too large".into()));
    }
    let text = std::str::from_utf8(signature).map_err(|_| UpdateError::BadSignature)?;
    let sig = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|_| UpdateError::BadSignature)?;
    let trusted = keys.iter().any(|k| {
        UnparsedPublicKey::new(&ED25519, k)
            .verify(manifest, &sig)
            .is_ok()
    });
    if !trusted {
        return Err(UpdateError::BadSignature);
    }
    let raw: Raw =
        serde_json::from_slice(manifest).map_err(|e| UpdateError::BadManifest(e.to_string()))?;
    let bad = |why: &str| UpdateError::BadManifest(why.to_owned());
    if raw.format != 2 {
        return Err(bad("unknown format"));
    }
    if raw.product != crate::consts::BIN_NAME {
        return Err(bad("another product"));
    }
    let version = Version::parse(&raw.version).ok_or_else(|| bad("version"))?;
    for a in &raw.artifacts {
        check_artifact(a)?;
    }
    Ok(Release {
        version,
        date: clean(&raw.date, 32),
        notes: clean(&raw.notes, MAX_NOTES),
        notes_url: raw.notes_url.filter(|u| safe_url(u, NOTES_PREFIX)),
        artifacts: raw.artifacts,
        manifest: manifest.to_vec(),
        signature: signature.to_vec(),
    })
}

fn check_artifact(a: &Artifact) -> Result<(), UpdateError> {
    let bad = |why: &str| UpdateError::BadManifest(format!("{}: {why}", a.path));
    if !BUNDLE_FILES.contains(&a.path.as_str()) {
        return Err(bad("unknown path"));
    }
    if !safe_url(&a.url, DOWNLOAD_PREFIX) {
        return Err(bad("url outside the repository's releases"));
    }
    if !is_sha256(&a.sha256) || !is_sha256(&a.unpacked_sha256) {
        return Err(bad("sha256"));
    }
    let max_unpacked = if a.is_folder() { MAX_FOLDER } else { MAX_FILE };
    if a.size == 0 || a.size > MAX_FILE || a.unpacked_size == 0 || a.unpacked_size > max_unpacked {
        return Err(bad("size"));
    }
    Ok(())
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Below `prefix`, and nothing a shell or `xdg-open` could misread.
fn safe_url(url: &str, prefix: &str) -> bool {
    url.starts_with(prefix)
        && url.len() <= 512
        && url
            .bytes()
            .all(|b| b.is_ascii_graphic() && !matches!(b, b'"' | b'\'' | b'\\' | b'`'))
        && !url.contains("..")
}

/// Control characters removed, whitespace collapsed, at most `max` characters.
fn clean(s: &str, max: usize) -> String {
    let words: Vec<&str> = s
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect();
    words.join(" ").chars().take(max).collect()
}

impl Release {
    /// The bundle's files for `arch`; each of `BUNDLE_FILES` must be there exactly once.
    pub fn files(&self, arch: &str) -> Result<Vec<&Artifact>, UpdateError> {
        BUNDLE_FILES
            .iter()
            .map(|path| {
                let mut found = self
                    .artifacts
                    .iter()
                    .filter(|a| a.channel == BUNDLE_CHANNEL && a.arch == arch && a.path == *path);
                match (found.next(), found.next()) {
                    (Some(a), None) => Ok(a),
                    _ => Err(UpdateError::bad_file(
                        path,
                        format!("no single file for {arch}"),
                    )),
                }
            })
            .collect()
    }

    /// Whether this release is newer than `running`; otherwise the refusal.
    pub fn ensure_newer(&self, running: &Version) -> Result<(), UpdateError> {
        if self.version > *running {
            Ok(())
        } else {
            Err(UpdateError::NotNewer {
                offered: self.version.to_string(),
                running: running.to_string(),
            })
        }
    }
}
