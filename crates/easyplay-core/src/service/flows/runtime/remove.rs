//! `RemoveRuntime` (RT-013): refused for external builds, the newest of a family and builds in use (SVC-008).
use crate::ids::RuntimeName;
use crate::model::{Game, RuntimeRef};
use crate::runtime::{Resolution, RuntimeService, RuntimeStore, gc};
use crate::service::prelude::*;
use crate::storage::{RuntimeEvent, RuntimeRepo};

impl AppService {
    pub(crate) fn remove_runtime(&mut self, name: RuntimeName) -> Result<()> {
        let catalog = self.deps.runtimes.catalog();
        let games: Vec<Game> = self
            .lib
            .snapshot()
            .games
            .iter()
            .filter_map(|g| self.lib.game(g.id).ok())
            .collect();
        let runtimes = Arc::clone(&self.deps.runtimes);
        let users = move |n: &RuntimeName| users_of(&*runtimes, &games, n);
        let store = RuntimeStore::new(&self.deps.paths);
        let path = gc::remove_build(
            &*self.deps.fs,
            &store,
            &catalog,
            &users,
            &name,
            self.deps.clock.now(),
        )?;
        tracing::info!(build = %name, path = %path.display(), "runtime removed");
        // The event is history only; the removal already happened.
        let _ = self.lib.db_mut().add_runtime_event(&RuntimeEvent {
            id: 0,
            at: self.deps.clock.now(),
            variant: None,
            kind: "removed".into(),
            summary_json: serde_json::json!({ "build": name.as_str() }).to_string(),
        });
        self.scan_runtimes();
        Ok(())
    }
}

/// The names of the games that resolve to `name` (pinned to it, or resolving to it now).
pub(crate) fn users_of(
    runtimes: &dyn RuntimeService,
    games: &[Game],
    name: &RuntimeName,
) -> Vec<String> {
    games
        .iter()
        .filter(|g| match &g.runtime {
            RuntimeRef::Pinned(n) => n == name,
            r => matches!(runtimes.resolve(r, Some(g)), Resolution::Ready(rt) if &rt.name == name),
        })
        .map(|g| g.name.clone())
        .collect()
}
