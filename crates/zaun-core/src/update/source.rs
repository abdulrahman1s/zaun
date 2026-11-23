//! Where updates come from and whom to trust (ADR-0040 §1–2). Everything derives from
//! [`REPO_URL`](crate::consts::REPO_URL).

use std::time::Duration;

use crate::artwork::http::AllowList;
use crate::consts::repo_url;

/// The signed manifest attached to every release (by the release workflow).
pub const MANIFEST_NAME: &str = "manifest.json";
/// Its detached signature: base64 of 64 Ed25519 bytes over the manifest's exact bytes.
pub const SIGNATURE_NAME: &str = "manifest.json.sig";

/// The manifest of the self-contained bundle (format 2, ADR-0041). `manifest.json`
/// (format 1) stays for copies from before it, which update their tarball in place.
pub const BUNDLE_MANIFEST_NAME: &str = "bundle.json";
/// Its signature.
pub const BUNDLE_SIGNATURE_NAME: &str = "bundle.json.sig";

/// The newest stable release's bundle manifest.
pub const MANIFEST_URL: &str = concat!(repo_url!(), "/releases/latest/download/bundle.json");
/// Its signature.
pub const SIGNATURE_URL: &str = concat!(repo_url!(), "/releases/latest/download/bundle.json.sig");
/// Every file a manifest names must be below this.
pub const DOWNLOAD_PREFIX: &str = concat!(repo_url!(), "/releases/download/");
/// A `notes_url` is kept only below this.
pub const NOTES_PREFIX: &str = concat!(repo_url!(), "/");

/// First requests to the repository's host only; GitHub serves release files from its
/// asset hosts by redirect (`release-assets.githubusercontent.com`, seen for v0.2.6).
pub const UPDATE_HOSTS: AllowList = AllowList {
    request: &["github.com"],
    redirect: &[
        "objects.githubusercontent.com",
        "release-assets.githubusercontent.com",
    ],
};

/// Public Ed25519 keys whose signatures are trusted (raw 32 bytes; more than one allows a
/// rotation). The release key's private half signs in CI (`ZAUN_UPDATE_SIGNING_KEY`,
/// RELEASE_STRATEGY §4.1). A build with an empty list never checks (`ZN-UPD-002`).
pub const TRUSTED_KEYS: &[[u8; 32]] = &[RELEASE_KEY];

/// The release key (2026-11-18), `c1bfd40d…`.
const RELEASE_KEY: [u8; 32] = [
    0xc1, 0xbf, 0xd4, 0x0d, 0xc5, 0xce, 0x8b, 0xe9, 0x35, 0x50, 0xa5, 0x13, 0xa9, 0x5e, 0x77, 0xca,
    0xfe, 0x18, 0xe8, 0x92, 0x4e, 0xed, 0x9e, 0x71, 0x56, 0x7a, 0x52, 0x52, 0xbf, 0xe9, 0x9f, 0xf5,
];

/// Largest manifest or signature read.
pub const MAX_MANIFEST: u64 = 64 * 1024;
/// Largest file of a release, downloaded or unpacked (one file of a folder, too).
pub const MAX_FILE: u64 = 128 * 1024 * 1024;
/// Largest unpacked folder (the Chat engine is about 240 MB).
pub const MAX_FOLDER: u64 = 1024 * 1024 * 1024;
/// Most files in one folder.
pub const MAX_FOLDER_FILES: usize = 1024;
/// Time budget of the manifest and its signature.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// Time budget of one file.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// Automatic checks at most this often.
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// The staged binary must answer `--version` within this.
pub const SMOKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The channel of `bundle.json` (format 2): one version folder of the bundle store.
/// `manifest.json` (format 1, channel `tarball`) is still published for copies from before
/// ADR-0041 and is never read by this one.
pub const BUNDLE_CHANNEL: &str = "bundle";
/// The bundle's files and folders, relative to a version folder. A path ending in `/` is a
/// folder, shipped as a gzip-compressed tar of plain files; it is shared by every version
/// whose copy has the same hash (`store::Store::component`).
pub const BUNDLE_FILES: [&str; 4] = ["zaun", "zaun-init", "zaun-overlay", "zaun-chat"];
/// Paths a manifest still carries for older copies, which this one accepts and skips.
/// `simplex/` is the old Chat engine's folder: copies up to 0.4.2 refuse a release without it,
/// so releases ship it as a one-file stub (ADR-0042).
pub const RETIRED_FILES: [&str; 1] = ["simplex/"];
/// The program a version folder starts with.
pub const BUNDLE_BINARY: &str = "zaun";
