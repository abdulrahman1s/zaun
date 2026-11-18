//! Opening a message's link, checked first when `chat.check_links` is on (ADR-0033).
use super::services::ServicesDone;
use crate::artwork::http::HttpClient;
use crate::jobs::JobLabel;
use crate::linkcheck::{self, CheckError, LinkStep, Verdict, WebLink};
use crate::service::flows::desktop::DesktopDone;
use crate::service::prelude::*;

impl AppService {
    /// The link check's client (directly, never through the GIF relay).
    fn link_http(&mut self) -> Option<Arc<HttpClient>> {
        let base = self.deps.http.as_ref()?;
        Some(Arc::clone(self.flow.services.link_http.get_or_insert_with(
            || Arc::new(base.for_hosts(linkcheck::HOSTS)),
        )))
    }

    pub(super) fn open_chat_link(&mut self, url: &str, check: bool) -> Result<()> {
        let Some(link) = WebLink::parse(url) else {
            tracing::info!("chat link not opened: not an http or https link");
            return Ok(());
        };
        let http = match linkcheck::step(check && self.prefs.chat.check_links, &link) {
            LinkStep::Open => return self.open_web_link(link),
            LinkStep::Check => self.link_http(),
            LinkStep::Ask => None,
        };
        let Some(http) = http else {
            self.ask_unchecked(&link);
            return Ok(());
        };
        self.services_job("chat-link-check", move || {
            let verdict = linkcheck::check(&http, &link);
            ServicesDone::Checked { link, verdict }
        });
        Ok(())
    }

    pub(super) fn on_link_checked(
        &self,
        link: WebLink,
        verdict: Result<Verdict, CheckError>,
    ) -> Result<()> {
        match verdict {
            Ok(Verdict::Safe | Verdict::Unknown) => self.open_web_link(link),
            Ok(Verdict::Unsafe) => {
                self.notice(Notice::ChatUnsafeLink {
                    url: link.url().to_owned(),
                    host: link.host().to_owned(),
                    unchecked: false,
                });
                Ok(())
            }
            Err(e) => {
                tracing::info!(error = %e, "link check gave no answer");
                self.ask_unchecked(&link);
                Ok(())
            }
        }
    }

    fn ask_unchecked(&self, link: &WebLink) {
        self.notice(Notice::ChatUnsafeLink {
            url: link.url().to_owned(),
            host: link.host().to_owned(),
            unchecked: true,
        });
    }

    fn open_web_link(&self, link: WebLink) -> Result<()> {
        let d = (self.deps.desktop.clone()).ok_or_else(|| Error::not_available("DESKTOP-005"))?;
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("chat-open-link"),
            move |_| {
                if let Err(e) = d.opener.open_web(&link) {
                    post(Internal::Desktop(Box::new(DesktopDone::Failed(e))));
                }
            },
        );
        Ok(())
    }
}
