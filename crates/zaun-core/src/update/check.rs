//! Fetch the newest release's manifest and signature and verify them (blocking; job pool or
//! CLI). Two plain GETs of static files: no identifiers, no query.

use super::UpdateError;
use super::manifest::{Release, verify};
use super::source::{MANIFEST_URL, MAX_MANIFEST, SIGNATURE_URL};
use crate::artwork::http::HttpClient;

/// The newest stable release, verified with `keys`. No request without a key.
pub fn fetch_release(http: &HttpClient, keys: &[[u8; 32]]) -> Result<Release, UpdateError> {
    if keys.is_empty() {
        return Err(UpdateError::NotConfigured);
    }
    let manifest = get(http, MANIFEST_URL)?;
    let signature = get(http, SIGNATURE_URL)?;
    verify(&manifest, &signature, keys)
}

fn get(http: &HttpClient, url: &str) -> Result<Vec<u8>, UpdateError> {
    let r = http.get_capped(url, MAX_MANIFEST)?;
    if r.status != 200 {
        return Err(UpdateError::Unreachable(format!(
            "{url}: status {}",
            r.status
        )));
    }
    Ok(r.body)
}
