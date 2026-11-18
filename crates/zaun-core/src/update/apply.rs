//! Put a staged update in place, and take it back (ADR-0040 §5). The caller makes sure no
//! game or tool session is open.

use serde::{Deserialize, Serialize};

use super::UpdateError;
use super::install::{Layout, file_name};
use super::manifest::{Release, verify};
use super::source::{
    MANIFEST_NAME, MAX_FILE, MAX_MANIFEST, SIGNATURE_NAME, SMOKE_TIMEOUT, TARBALL_CHANNEL,
    TARBALL_FILES,
};
use super::stage::{clear_dir, sha256_hex};
use super::version::Version;
use crate::fsx::{Fs, FsErrorKind};
use crate::host::{CommandRunner, CommandSpec};

/// The new version's trial: started this many times without being confirmed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trial {
    pub version: String,
    pub previous: String,
    pub starts: u32,
}

/// The staged release, verified again from disk; `None` when nothing is staged.
pub fn staged_release(
    fs: &dyn Fs,
    layout: &Layout,
    keys: &[[u8; 32]],
) -> Result<Option<Release>, UpdateError> {
    let dir = layout.staged();
    let manifest = match fs.read(&dir.join(MANIFEST_NAME), MAX_MANIFEST) {
        Ok(b) => b,
        Err(e) if e.kind == FsErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let signature = fs.read(&dir.join(SIGNATURE_NAME), MAX_MANIFEST)?;
    verify(&manifest, &signature, keys).map(Some)
}

/// Apply the staged update: verify, smoke-test, keep the current files, replace them, start
/// the trial. Returns the new version; `None` when nothing is staged.
pub fn apply(
    fs: &dyn Fs,
    runner: &dyn CommandRunner,
    layout: &Layout,
    keys: &[[u8; 32]],
    running: &Version,
    arch: &str,
) -> Result<Option<Version>, UpdateError> {
    let Some(release) = staged_release(fs, layout, keys)? else {
        return Ok(None);
    };
    let staged = layout.staged();
    let checked = release.ensure_newer(running).and_then(|()| {
        for a in release.files(TARBALL_CHANNEL, arch)? {
            let name = file_name(&a.path);
            let data = fs.read(&staged.join(name), MAX_FILE)?;
            if sha256_hex(&data) != a.unpacked_sha256 {
                return Err(UpdateError::bad_file(name, "staged file was changed"));
            }
        }
        smoke_test(
            runner,
            &staged.join(file_name(TARBALL_FILES[1])),
            &release.version,
        )
    });
    if let Err(e) = checked {
        // A staged update that cannot be applied is dropped, so it is not tried at every start.
        let _ = clear_dir(fs, &staged); // best effort: the error below is what matters
        return Err(e);
    }
    swap(fs, layout)?;
    let trial = Trial {
        version: release.version.to_string(),
        previous: running.to_string(),
        starts: 0,
    };
    write_trial(fs, layout, &trial)?;
    clear_dir(fs, &staged)?;
    Ok(Some(release.version))
}

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

/// Keep each current file in `previous/`, then rename each staged file over it. A failure
/// half-way puts the files already replaced back.
fn swap(fs: &dyn Fs, layout: &Layout) -> Result<(), UpdateError> {
    let previous = layout.previous();
    clear_dir(fs, &previous)?;
    fs.create_dir_all(&previous, 0o755)?;
    for rel in TARBALL_FILES {
        fs.copy_file(&layout.target(rel), &previous.join(file_name(rel)), 0o755)?;
    }
    let mut done: Vec<&str> = Vec::new();
    for rel in TARBALL_FILES {
        if let Err(e) = fs.rename(&layout.staged().join(file_name(rel)), &layout.target(rel)) {
            for back in done {
                // Best effort: the error below is what the user sees.
                let _ = fs.rename(&previous.join(file_name(back)), &layout.target(back));
            }
            return Err(e.into());
        }
        done.push(rel);
    }
    Ok(())
}

pub(super) fn write_trial(fs: &dyn Fs, layout: &Layout, t: &Trial) -> Result<(), UpdateError> {
    let bytes = serde_json::to_vec(t).map_err(|e| UpdateError::Install(e.to_string()))?;
    fs.atomic_write(&layout.trial_file(), &bytes, 0o644)?;
    Ok(())
}

/// Put `previous/` back over the installed files and end the trial.
pub fn rollback(fs: &dyn Fs, layout: &Layout) -> Result<(), UpdateError> {
    let previous = layout.previous();
    for rel in TARBALL_FILES {
        let kept = previous.join(file_name(rel));
        if fs.exists(&kept) {
            fs.rename(&kept, &layout.target(rel))?;
        }
    }
    clear_dir(fs, &previous)?;
    remove_if_present(fs, &layout.trial_file())
}

pub(super) fn remove_if_present(fs: &dyn Fs, path: &std::path::Path) -> Result<(), UpdateError> {
    match fs.remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind == FsErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
