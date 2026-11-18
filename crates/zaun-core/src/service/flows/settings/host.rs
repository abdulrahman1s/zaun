//! Host facts: probe on the job pool, publish, and rebuild the host-dependent policy check (SVC-009).
use std::sync::PoisonError;

use crate::host::HostFacts;
use crate::library::service::HostPolicyCheck;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn probe_host(&self, fresh: bool) {
        let host = Arc::clone(&self.deps.host);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("host-probe"),
            move |_| {
                let f = if fresh { host.refresh() } else { host.facts() };
                post(Internal::HostFacts(f));
            },
        );
    }

    pub(crate) fn on_host_facts(&mut self, f: Arc<HostFacts>) -> Result<()> {
        self.host = Some(Arc::clone(&f));
        self.install_policy_check();
        self.hub.host.store(Arc::new(Some((*f).clone())));
        self.emit(Event::HostFactsChanged);
        // The full Doctor report follows the first probe and every re-probe.
        self.run_doctor();
        Ok(())
    }

    /// The host-dependent policy check, with the current Developer Mode preference; called
    /// when the host facts or the preference change.
    pub(crate) fn install_policy_check(&mut self) {
        let Some(f) = self.host.clone() else { return };
        if self.deps.policy_check.is_some() {
            return;
        }
        let e = &self.deps.launch_env;
        let check = HostPolicyCheck {
            fs: Arc::clone(&self.deps.fs),
            mounts: e.mounts.clone(),
            paths: self.deps.paths.clone(),
            host: (*f).clone(),
            home: e.home.clone(),
            uid: e.uid,
            user_name: e.user_name.clone(),
            user_dirs: e.user_dirs.clone(),
            developer_mode: self.prefs.advanced.developer_mode,
            global_env: self.prefs.launch.global_env.clone(),
        };
        *self
            .check
            .inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(check));
    }
}
