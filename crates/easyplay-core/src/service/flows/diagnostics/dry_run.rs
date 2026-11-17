//! `Inspect` of the next launch: the dry run, which executes no host action and writes nothing (DEV-001).
use crate::launch::{
    self, InspectorData, InspectorSource, LaunchError, LaunchRequest, OrchestratorDeps,
};
use crate::service::prelude::*;

impl AppService {
    pub(super) fn inspect_next(
        &mut self,
        id: GameId,
        exe: Option<ExecutableId>,
        reply_to: QueryId,
    ) {
        let describe = self.describer(Some(id));
        let game = match self.lib.game(id) {
            Ok(g) => g,
            Err(e) => return self.query_failed(reply_to, describe(&e)),
        };
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        let paths = self.deps.paths.clone();
        let mut env = self.deps.launch_env.clone();
        let developer = crate::sandbox::validate::effective_developer_mode(
            self.prefs.advanced.developer_mode,
            &game.policy,
            &game.launch,
        );
        let install_id = self.install_id.clone();
        let other_roots: Vec<_> = self.lib.exe_index().other_roots(Some(id));
        let runtimes = Arc::clone(&self.deps.runtimes);
        let launcher = Arc::clone(&self.deps.launcher);
        let locks = Arc::clone(&self.deps.locks);
        let host = Arc::clone(&self.deps.host);
        env.global_env = self.prefs.launch.global_env.clone();
        self.answer_on_pool("inspect-dry-run", reply_to, move |_| {
            let facts = host.facts();
            let mut henv = env.host_env(&paths, &install_id, &other_roots, &*fs);
            henv.developer_mode = developer;
            let deps = OrchestratorDeps {
                host: &facts,
                env: &henv,
                runtimes: &*runtimes,
                launcher: &*launcher,
                locks: &*locks,
                fs: Arc::clone(&fs),
                clock: &*clock,
                steps: None,
                cancel: None,
            };
            let req = LaunchRequest {
                game: &game,
                exe,
                extra_args: &[],
                session_id: SessionId::generate(&*clock),
                network_consent: false,
            };
            match launch::dry_run(&req, &deps) {
                Ok(d) => {
                    let mut notes: Vec<String> = d.note.into_iter().collect();
                    notes.extend(d.planned.sandbox.notes.iter().map(|n| n.text.clone()));
                    if d.audit_first {
                        notes.push(
                            "The shared runtime is audited before the game starts.".to_owned(),
                        );
                    }
                    let explain = crate::sandbox::explain(&d.planned.sandbox, &game.policy);
                    QueryResult::Inspector(Box::new(InspectorData {
                        source: InspectorSource::DryRun,
                        plan: d.planned.plan,
                        sandbox: Some(d.planned.sandbox),
                        explain,
                        timeline: Vec::new(),
                        session: None,
                        notes,
                    }))
                }
                Err(e) => {
                    let err = match e {
                        LaunchError::Failed(e) => e,
                        other => err!(IntTaskFailed, detail = format!("{other:?}")),
                    };
                    QueryResult::Failed(Box::new(describe(&err)))
                }
            }
        });
    }
}
