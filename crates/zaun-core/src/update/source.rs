//! Where updates come from and whom to trust (ADR-0040 §1–2). Everything derives from
//! [`REPO_URL`](crate::consts::REPO_URL).

use std::time::Duration;

use crate::artwork::http::AllowList;
use crate::consts::repo_url;

/// The signed manifest attached to every release (by the release workflow).
pub const MANIFEST_NAME: &str = "manifest.json";
/// Its detached signature: base64 of 64 Ed25519 bytes over the manifest's exact bytes.
pub const SIGNATURE_NAME: &str = "manifest.json.sig";

/// The newest stable release's manifest.
pub const MANIFEST_URL: &str = concat!(repo_url!(), "/releases/latest/download/manifest.json");
/// Its signature.
pub const SIGNATURE_URL: &str = concat!(repo_url!(), "/releases/latest/download/manifest.json.sig");
/// Every file a manifest names must be below this.
pub const DOWNLOAD_PREFIX: &str = concat!(repo_url!(), "/releases/download/");
/// A `notes_url` is kept only below this.
pub const NOTES_PREFIX: &str = concat!(repo_url!(), "/");

/// First requests to the repository's host only; GitHub serves release files from its
/// asset hosts by redirect. [Unverified: no release existed when this was written; check the
/// first release's redirects against it.]
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
/// Largest file of a release, downloaded or unpacked.
pub const MAX_FILE: u64 = 128 * 1024 * 1024;
/// Time budget of the manifest and its signature.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// Time budget of one file.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// Automatic checks at most this often.
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// The staged binary must answer `--version` within this.
pub const SMOKE_TIMEOUT: Duration = Duration::from_secs(10);

/// The release channel of an owned install.
pub const TARBALL_CHANNEL: &str = "tarball";
/// The files of the tarball channel, relative to the install prefix, in the order they are
/// replaced (the shim before the binary that uses it).
pub const TARBALL_FILES: [&str; 2] = ["libexec/zaun/zaun-init", "libexec/zaun/zaun"];
