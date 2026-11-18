//! What the service needs to check for and install updates (`ServiceDeps::updates`).

use std::sync::Arc;

use super::install::Install;
use super::source::{
    CHECK_TIMEOUT, DOWNLOAD_TIMEOUT, MAX_FILE, MAX_MANIFEST, TRUSTED_KEYS, UPDATE_HOSTS,
};
use crate::artwork::http::HttpClient;
use crate::host::CommandRunner;

pub struct UpdateDeps {
    /// The manifest client ([`UpdateDeps::check_client`]).
    pub check: Arc<HttpClient>,
    /// The files client ([`UpdateDeps::files_client`]).
    pub files: Arc<HttpClient>,
    /// Trusted Ed25519 keys: [`TRUSTED_KEYS`] in a build, a test key in tests.
    pub keys: Vec<[u8; 32]>,
    pub install: Install,
    /// Smoke-tests the staged binary.
    pub runner: Arc<dyn CommandRunner>,
    /// `std::env::consts::ARCH` in a build.
    pub arch: String,
}

impl UpdateDeps {
    /// The production seams: `base`'s transport and clock, the compiled-in keys.
    pub fn new(base: &HttpClient, install: Install, runner: Arc<dyn CommandRunner>) -> Self {
        Self {
            check: Arc::new(Self::check_client(base)),
            files: Arc::new(Self::files_client(base)),
            keys: TRUSTED_KEYS.to_vec(),
            install,
            runner,
            arch: std::env::consts::ARCH.to_owned(),
        }
    }

    /// [`UPDATE_HOSTS`] for the manifest and its signature: small and quick.
    pub fn check_client(base: &HttpClient) -> HttpClient {
        base.for_downloads(UPDATE_HOSTS, MAX_MANIFEST, CHECK_TIMEOUT)
    }

    /// [`UPDATE_HOSTS`] for a release's files: up to [`MAX_FILE`] in [`DOWNLOAD_TIMEOUT`].
    pub fn files_client(base: &HttpClient) -> HttpClient {
        base.for_downloads(UPDATE_HOSTS, MAX_FILE, DOWNLOAD_TIMEOUT)
    }
}
