//! The diagnostics report: preview, save as an archive, saved notice (ERR-003).
use crate::diagnostics::report::{self, GamePart, ReportFile, ReportInput};
use crate::model::SessionKind;
use crate::service::prelude::*;

impl AppService {
    /// `BuildReport`: collect the files for the preview.
    pub(crate) fn build_report(
        &mut self,
        game: Option<GameId>,
        session: Option<SessionId>,
        reply_to: QueryId,
    ) -> Result<()> {
        let g = game.and_then(|id| self.lib.game(id).ok());
        let session = session.or_else(|| {
            let id = g.as_ref()?.id;
            self.lib
                .db_mut()
                .session_history(Some(id), &[SessionKind::Game], 0, 1)
                .ok()
                .and_then(|v| v.first().map(|s| s.id))
        });
        let fs = Arc::clone(&self.deps.fs);
        let paths = self.deps.paths.clone();
        let redactor = self.redactor();
        let doctor = self.hub.diagnostics.load().doctor.clone();
        let facts = self.host.clone();
        self.answer_on_pool("build-report", reply_to, move |_| {
            let version = format!("EasyPlay {}\nreport format 1\n", crate::consts::APP_VERSION);
            let files = report::build(&ReportInput {
                fs: &*fs,
                paths: &paths,
                redactor: &redactor,
                version,
                doctor: doctor.as_ref(),
                facts: facts.as_deref(),
                game: g.as_ref().map(|g| GamePart {
                    policy: &g.policy,
                    launch: &g.launch,
                    session,
                }),
            });
            QueryResult::Report(files)
        });
        Ok(())
    }

    /// `SaveReport`: write the archive where the user chose.
    pub(crate) fn save_report(&mut self, files: Vec<ReportFile>, dest: PathBuf) -> Result<()> {
        let fs = Arc::clone(&self.deps.fs);
        let mtime = u64::try_from(self.deps.clock.now().as_millis() / 1000).unwrap_or(0);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("save-report"),
            move |_| {
                let r = report::archive_bytes(&files, mtime)
                    .map_err(|e| err!(FsWriteDenied).with_source(e))
                    .and_then(|bytes| fs.atomic_write(&dest, &bytes, 0o600).map_err(Error::from))
                    .map(|()| dest);
                post(Internal::ReportSaved(r));
            },
        );
        Ok(())
    }

    pub(crate) fn on_report_saved(&mut self, r: Result<PathBuf>) -> Result<()> {
        let path = r?;
        self.notice(Notice::ReportSaved { path });
        Ok(())
    }
}
