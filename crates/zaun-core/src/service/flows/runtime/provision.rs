//! `ProvisionRuntime`: install (or update, for a channel) a build and its runtime in a Provision session (SVC-008).
use super::RuntimeJobDone;
use crate::launch::{LocalTrustedSteps, TrustedProgress, TrustedSteps, plan_provision};
use crate::model::RuntimeRef;
use crate::runtime::provision::{check_postconditions, interpret};
use crate::runtime::{ProvisionNeed, Resolution, RuntimeStore};
use crate::service::prelude::*;

impl AppService {
    /// The trusted-session runner for launches and runtime commands.
    pub(crate) fn trusted_steps(&self) -> LocalTrustedSteps {
        LocalTrustedSteps {
            launcher: Arc::clone(&self.deps.launcher),
            fs: Arc::clone(&self.deps.fs),
            clock: Arc::clone(&self.deps.clock),
            proc: Arc::clone(&self.deps.proc),
            paths: self.deps.paths.clone(),
            store: RuntimeStore::new(&self.deps.paths),
            prepare_online: self.prefs.protection.prepare_online,
        }
    }

    pub(crate) fn provision_runtime(&mut self, target: RuntimeRef) -> Result<()> {
        let Some(need) = self.provision_need(&target)? else {
            // Already installed: nothing to download.
            self.emit(Event::RuntimesChanged);
            return Ok(());
        };
        let steps = self.trusted_steps();
        let runtimes = Arc::clone(&self.deps.runtimes);
        let host = Arc::clone(&self.deps.host);
        let env = self.deps.launch_env.clone();
        let paths = self.deps.paths.clone();
        let install_id = self.install_id.clone();
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("runtime-provision"),
            move |cancel| {
                let r = (|| -> Result<()> {
                    let facts = host.facts();
                    let henv = env.host_env(&paths, &install_id, &[], &*fs);
                    let sid = SessionId::generate(&*clock);
                    let planned = plan_provision(&need, &runtimes.catalog(), &facts, &henv, sid)?;
                    let end = steps.run(&planned, true, &mut log_progress, cancel)?;
                    interpret(&end)?;
                    steps.after_install()?;
                    let catalog = runtimes
                        .refresh()
                        .wait()
                        .map_err(|e| err!(IntTaskFailed, detail = e.to_string()))?;
                    check_postconditions(&catalog, &need)?;
                    Ok(())
                })();
                let notices = r.err().into_iter().collect();
                post(Internal::RuntimeJob(RuntimeJobDone { notices }));
            },
        );
        Ok(())
    }

    /// What `target` still needs installed, or `None` when it is there.
    fn provision_need(&self, target: &RuntimeRef) -> Result<Option<ProvisionNeed>> {
        Ok(match target {
            RuntimeRef::Channel(c) => Some(ProvisionNeed::Codename(*c)),
            other => match self.deps.runtimes.resolve(other, None) {
                Resolution::NeedsProvision(n) => Some(n),
                Resolution::Ready(_) => None,
                Resolution::Unavailable(r) => {
                    return Err(Error::new(r.code()).with("reason", format!("{r:?}")));
                }
            },
        })
    }
}

fn log_progress(p: TrustedProgress) {
    match p {
        TrustedProgress::Waiting => {
            tracing::info!("provision queued behind a running session");
        }
        TrustedProgress::Phase(p) => tracing::info!(phase = ?p, "provision"),
        TrustedProgress::Bytes { bytes, rate } => {
            tracing::debug!(bytes, rate, "provision download");
        }
    }
}
