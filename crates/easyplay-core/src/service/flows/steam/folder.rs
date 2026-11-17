//! Open a Steam game's install folder or Windows storage (ADR-0031).
use crate::service::prelude::*;

use crate::desktop::KnownPath;
use crate::service::flows::desktop::DesktopDone;

impl AppService {
    pub(super) fn steam_open_folder(&mut self, id: GameId, prefix: bool) -> Result<()> {
        let s = self.steam_summary(id)?;
        let Some(f) = s.steam() else { return Ok(()) };
        let path: PathBuf = if prefix {
            f.prefix().ok_or_else(|| err!(StmNoPrefix))?.to_path_buf()
        } else {
            f.install_dir.clone()
        };
        let Some(d) = self.deps.desktop.clone() else {
            return Err(Error::not_available("DESKTOP-005"));
        };
        let fs = Arc::clone(&self.deps.fs);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("desktop-open"),
            move |_| {
                let done = if !fs.metadata_follow(&path).is_ok_and(|m| m.is_dir()) {
                    DesktopDone::Failed(err!(FsReadDenied, path = path.display()))
                } else {
                    match d.opener.open(&KnownPath::folder(path)) {
                        Ok(()) => DesktopDone::Done,
                        Err(e) => DesktopDone::Failed(e),
                    }
                };
                post(Internal::Desktop(Box::new(done)));
            },
        );
        Ok(())
    }
}
