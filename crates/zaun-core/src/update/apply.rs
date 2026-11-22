//! Switch to a staged version, and back (ADR-0041 §4). The caller makes sure no game or
//! tool session is open. Switching moves links only: the running program and every version
//! folder stay as they are.

use serde::{Deserialize, Serialize};

use super::UpdateError;
use super::manifest::{Release, verify};
use super::source::{
    BUNDLE_MANIFEST_NAME, BUNDLE_SIGNATURE_NAME, MAX_FILE, MAX_MANIFEST, SMOKE_TIMEOUT,
};
use super::stage::sha256_hex;
use super::store::{Store, clear_dir, remove_if_present};
use super::version::Version;
use crate::fsx::{Fs, FsErrorKind};
use crate::host::{CommandRunner, CommandSpec};

/// The trial of a newly switched-to version: started this many times without a
/// confirmation. `previous` is `None` when the installed copy came before it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trial {
    pub version: String,
    pub previous: Option<String>,
    pub starts: u32,
}

/// The staged release and its folder, verified again from disk; `None` when nothing is
/// staged.
pub fn staged_release(
    fs: &dyn Fs,
    store: &Store,
    keys: &[[u8; 32]],
) -> Result<Option<(Release, std::path::PathBuf)>, UpdateError> {
    let Some((v, dir)) = store.linked(fs, &store.staged()) else {
        return Ok(None);
    };
    let manifest = match fs.read(&dir.join(BUNDLE_MANIFEST_NAME), MAX_MANIFEST) {
        Ok(b) => b,
        Err(e) if e.kind == FsErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let signature = fs.read(&dir.join(BUNDLE_SIGNATURE_NAME), MAX_MANIFEST)?;
    let release = verify(&manifest, &signature, keys)?;
    if release.version != v {
        return Err(UpdateError::Install(format!(
            "the folder of {v} holds {}",
            release.version
        )));
    }
    Ok(Some((release, dir)))
}

/// Switch to the staged version: verify, smoke-test, start its trial, move the links.
/// Returns the new version; `None` when nothing is staged.
pub fn apply(
    fs: &dyn Fs,
    runner: &dyn CommandRunner,
    store: &Store,
    keys: &[[u8; 32]],
    running: &Version,
    arch: &str,
) -> Result<Option<Version>, UpdateError> {
    let Some((release, dir)) = staged_release(fs, store, keys)? else {
        return Ok(None);
    };
    let current = store.linked(fs, &store.current()).map(|(v, _)| v);
    let checked = release
        .ensure_newer(running)
        .and_then(|()| match &current {
            Some(c) => release.ensure_newer(c),
            None => Ok(()),
        })
        .and_then(|()| check_folder(fs, &release, &dir, arch))
        .and_then(|()| smoke_test(runner, &Store::binary(&dir), &release.version));
    if let Err(e) = checked {
        // A staged version that cannot be used is dropped, so it is not tried at every start.
        let _ = remove_if_present(fs, &store.staged()); // best effort: `e` is what matters
        let _ = clear_dir(fs, &dir); // as above
        return Err(e);
    }
    // The trial first: a new version must never be current without one.
    let trial = Trial {
        version: release.version.to_string(),
        previous: current.as_ref().map(Version::to_string),
        starts: 0,
    };
    write_trial(fs, store, &trial)?;
    let switched = match &current {
        Some(c) => store.set_link(fs, &store.previous(), c),
        None => remove_if_present(fs, &store.previous()),
    }
    .and_then(|()| store.set_link(fs, &store.current(), &release.version));
    if let Err(e) = switched {
        let _ = remove_if_present(fs, &store.trial_file()); // best effort; `e` is what matters
        return Err(e);
    }
    remove_if_present(fs, &store.staged())?;
    Ok(Some(release.version))
}

/// Every file of the version folder against its signed hash; every folder link resolves.
fn check_folder(
    fs: &dyn Fs,
    release: &Release,
    dir: &std::path::Path,
    arch: &str,
) -> Result<(), UpdateError> {
    for a in release.files(arch)? {
        let path = dir.join(a.name());
        if a.is_folder() {
            if !fs.metadata_follow(&path).is_ok_and(|m| m.is_dir()) {
                return Err(UpdateError::bad_file(
                    a.name(),
                    "the shared folder is missing",
                ));
            }
            continue;
        }
        let data = fs.read(&path, MAX_FILE)?;
        if sha256_hex(&data) != a.unpacked_sha256 {
            return Err(UpdateError::bad_file(a.name(), "staged file was changed"));
        }
    }
    Ok(())
}

/// `zaun --version` with an empty environment (`CommandRunner` clears it), as the session
/// supervisor runs: a missing program interpreter or library fails here, not at a start.
fn smoke_test(
    runner: &dyn CommandRunner,
    bin: &std::path::Path,
    v: &Version,
) -> Result<(), UpdateError> {
    let out = runner
        .run(&CommandSpec::new(bin, &["--version"]).timeout(SMOKE_TIMEOUT))
        .map_err(|e| UpdateError::Install(format!("the new version did not run: {e}")))?;
    let text = out.stdout_text();
    if out.success() && text.contains(&v.to_string()) {
        Ok(())
    } else {
        Err(UpdateError::Install(format!(
            "the new version answered {:?} (status {})",
            text.trim(),
            out.status
        )))
    }
}

pub(super) fn read_trial(fs: &dyn Fs, store: &Store) -> Option<Trial> {
    let b = fs.read(&store.trial_file(), 4096).ok()?;
    serde_json::from_slice(&b).ok()
}

pub(super) fn write_trial(fs: &dyn Fs, store: &Store, t: &Trial) -> Result<(), UpdateError> {
    let bytes = serde_json::to_vec(t).map_err(|e| UpdateError::Install(e.to_string()))?;
    fs.atomic_write(&store.trial_file(), &bytes, 0o644)?;
    Ok(())
}

/// Go back to the version before `current` (or to the installed copy) and end the trial.
/// Returns the version that failed.
pub fn rollback(fs: &dyn Fs, store: &Store) -> Result<Option<Version>, UpdateError> {
    let failed = store.linked(fs, &store.current()).map(|(v, _)| v);
    match store.linked(fs, &store.previous()) {
        Some((p, _)) => store.set_link(fs, &store.current(), &p)?,
        None => remove_if_present(fs, &store.current())?,
    }
    remove_if_present(fs, &store.previous())?;
    remove_if_present(fs, &store.trial_file())?;
    if let Some(v) = &failed {
        let _ = clear_dir(fs, &store.version_dir(v)); // best effort; pruned later otherwise
    }
    Ok(failed)
}
