//! Open the newest version's release notes (a page of the repository) in the browser.

use crate::linkcheck::WebLink;
use crate::service::flows::desktop::DesktopDone;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn open_release_notes(&mut self) -> Result<()> {
        let url = self
            .flow
            .updates
            .file
            .latest
            .as_ref()
            .and_then(|l| l.notes_url.clone());
        let link = url
            .as_deref()
            .and_then(WebLink::parse)
            .ok_or_else(|| err!(UpdNotNewer))?;
        self.desktop_job("update-notes", move |d| match d.opener.open_web(&link) {
            Err(e) => DesktopDone::Failed(e),
            Ok(()) => DesktopDone::Done,
        })
    }
}
