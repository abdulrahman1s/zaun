//! Classify, fingerprint and match one path on the job pool (SVC-003, LOGIC_SPECS §1).
use super::draft::DraftInputs;
use crate::consts::MIN_PE_SIZE;
use crate::fsx::{Fs, MountTable};
use crate::identity::fingerprint;
use crate::identity::matcher::{self, MatchOutcome, PathProbe};
use crate::identity::pe::{PeError, PeLimits};
use crate::library::GameDraft;
use crate::model::{Fingerprint, PeInfo};
use crate::service::prelude::*;

/// A Windows program, fingerprinted and matched.
pub(crate) struct Classified {
    pub canonical: PathBuf,
    pub fingerprint: Fingerprint,
    pub pe: PeInfo,
    pub outcome: MatchOutcome,
    /// Built when the outcome is `Unknown`.
    pub draft: Option<Box<GameDraft>>,
}

struct FsProbe<'a> {
    fs: &'a dyn Fs,
    mounts: &'a MountTable,
}

impl PathProbe for FsProbe<'_> {
    fn exists(&self, path: &Path) -> bool {
        self.fs.exists(path)
    }
    fn is_mounted(&self, mount_point: &Path) -> bool {
        self.mounts
            .mounts
            .iter()
            .any(|m| m.mount_point == mount_point)
    }
}

impl DraftInputs {
    /// Steps "Classify", "Fingerprint", "Match" and, for unknown programs, "Draft".
    pub(crate) fn classify(&self, path: &Path) -> Result<Classified> {
        ensure!(
            path.is_absolute(),
            IdNotWindowsProgram,
            path = path.display()
        );
        let canonical = self.fs.canonicalize(path)?;
        let md = self.fs.metadata(&canonical)?;
        ensure!(
            md.is_file() && md.size >= MIN_PE_SIZE,
            IdNotWindowsProgram,
            path = path.display()
        );
        let fp = fingerprint::compute(&*self.fs, &canonical)?;
        let pe = match self.pe.inspect(&canonical, &PeLimits::default()) {
            Ok(i) => i,
            Err(PeError::NotExecutable | PeError::NotPe(_)) if !is_msi(&canonical) => {
                bail!(IdNotWindowsProgram, path = path.display());
            }
            Err(_) => PeInfo::unknown(),
        };
        let probe = FsProbe {
            fs: &*self.fs,
            mounts: &self.mounts,
        };
        let outcome = matcher::resolve(&canonical, &fp, &pe, &self.index, &probe);
        let draft = match outcome {
            MatchOutcome::Unknown { .. } => Some(Box::new(self.draft(&canonical)?)),
            _ => None,
        };
        Ok(Classified {
            canonical,
            fingerprint: fp,
            pe,
            outcome,
            draft,
        })
    }
}

fn is_msi(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("msi"))
}
