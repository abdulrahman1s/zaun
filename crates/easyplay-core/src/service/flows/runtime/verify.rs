//! `VerifyRuntime` and the background audit after Shared-mode sessions and at start (SVC-008, RUNTIME_MANAGEMENT §6.3).
use crate::launch::TrustedSteps;
use crate::model::SlrVariant;
use crate::runtime::AuditDue;
use crate::service::prelude::*;

use super::RuntimeJobDone;

/// Background audits running (one at a time).
static AUDIT_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

impl AppService {
    /// "Verify now": audit every installed variant.
    pub(crate) fn verify_runtimes(&mut self) -> Result<()> {
        self.audit_runtimes(true);
        Ok(())
    }

    /// Audit the variants with `pending = 1` in the background, or every installed variant
    /// when `all`.
    pub(crate) fn audit_runtimes(&mut self, all: bool) {
        let steps = self.trusted_steps();
        let catalog = self.hub.runtimes.load();
        let variants: Vec<SlrVariant> = [SlrVariant::Steamrt3, SlrVariant::Steamrt4]
            .into_iter()
            .filter(|v| all || steps.audit_due(*v) == AuditDue::Pending)
            .filter(|v| !all || catalog.slr(*v).is_some_and(|s| s.is_complete()))
            .collect();
        if variants.is_empty() {
            return;
        }
        let post = self.poster();
        let priority = if all {
            JobPriority::Interactive
        } else {
            JobPriority::Background
        };
        let _ = self
            .deps
            .jobs
            .submit(priority, JobLabel("runtime-audit"), move |cancel| {
                let _one = AUDIT_LOCK.lock();
                let mut done = RuntimeJobDone::default();
                for v in variants {
                    match steps.audit(v, cancel) {
                        Ok(action) => done.notices.extend(action.notice()),
                        Err(e) => done.notices.push(e),
                    }
                }
                post(Internal::RuntimeJob(done));
            });
    }
}
