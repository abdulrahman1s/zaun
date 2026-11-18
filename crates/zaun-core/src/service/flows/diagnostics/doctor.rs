//! `RunDoctor`: every check, including the database and the init self-test (DEV-003, ERR-002).
use crate::diagnostics::{self, DoctorProbes};
use crate::host::{DoctorContext, DoctorReport};
use crate::service::prelude::*;
use crate::service::snapshot::Diagnostics;

impl AppService {
    /// Update the diagnostics snapshot and announce it.
    pub(crate) fn update_diagnostics(&self, f: impl FnOnce(&mut Diagnostics)) {
        let mut d = (*self.hub.diagnostics.load()).clone();
        f(&mut d);
        self.hub.diagnostics.store(Arc::new(d));
        self.emit(Event::DiagnosticsChanged);
    }

    pub(crate) fn run_doctor(&mut self) {
        let Some(facts) = self.host.clone() else {
            // The probe is still running; it triggers the Doctor when it lands.
            return;
        };
        let fs = Arc::clone(&self.deps.fs);
        let runner = self.deps.host.runner();
        let paths = self.deps.paths.clone();
        let uid = self.deps.launch_env.uid;
        let init = self.deps.launch_env.init_binary.clone();
        let catalog = self.hub.runtimes.load();
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Background, JobLabel("doctor"), move |_| {
                let ctx = DoctorContext {
                    fs: &*fs,
                    paths: &paths,
                    uid,
                    init_candidates: vec![init],
                };
                let mut r = diagnostics::doctor_report(&facts, &ctx, &catalog);
                diagnostics::complete(
                    &mut r,
                    &ctx,
                    &DoctorProbes {
                        runner: runner.as_deref(),
                        db_file: &paths.db_file,
                    },
                );
                post(Internal::Doctor(Box::new(r)));
            });
    }

    pub(crate) fn on_doctor(&mut self, report: DoctorReport) -> Result<()> {
        self.update_diagnostics(|d| d.doctor = Some(report));
        Ok(())
    }
}
