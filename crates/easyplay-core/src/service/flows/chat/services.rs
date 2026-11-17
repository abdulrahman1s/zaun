//! Chat's web services (ADR-0033): GIF search ([`crate::gifs`]) and link checks
//! ([`crate::linkcheck`]).
//!
//! - Every request runs on the job pool through the allow-listed HTTP client, derived from
//!   the artwork client with the service's own host list. Without that client (tests, a
//!   build without network) GIF search answers "unavailable" and links open unchecked
//!   only when the check is off.
//! - Picker pictures are `Background` jobs, so they never hold up the user's own actions.
use crate::artwork::http::HttpClient;
use crate::clock::MonoInstant;
use crate::gifs::GifAnswer;
use crate::jobs::JobLabel;
use crate::linkcheck::{CheckError, Verdict, WebLink};
use crate::service::prelude::*;

/// What this module keeps between messages.
#[derive(Default)]
pub(crate) struct ServicesState {
    /// The GIF client and the `chat.gif_proxy` it was made for.
    pub(super) gif_http: Option<(String, Arc<HttpClient>)>,
    pub(super) link_http: Option<Arc<HttpClient>>,
    /// Trending, when, and for which key and relay.
    pub(super) trending: Option<(MonoInstant, String, GifAnswer)>,
    pub(super) sent: u64,
}

/// Completions of this module's jobs.
pub(crate) enum ServicesDone {
    /// Trending arrived (kept, then answered); `with` is the key and relay it was asked with.
    Trending {
        reply_to: QueryId,
        with: String,
        answer: GifAnswer,
    },
    /// A GIF is ready to send.
    GifReady { chat: ChatRef, path: PathBuf },
    /// A GIF could not be sent.
    GifFailed,
    /// The link check answered (or not).
    Checked {
        link: WebLink,
        verdict: Result<Verdict, CheckError>,
    },
}

impl AppService {
    pub(super) fn services_job(
        &self,
        label: &'static str,
        f: impl FnOnce() -> ServicesDone + Send + 'static,
    ) {
        self.pool(label, JobPriority::Interactive, move || {
            FlowDone::ChatServices(f())
        });
    }

    pub(super) fn pool(
        &self,
        label: &'static str,
        priority: JobPriority,
        f: impl FnOnce() -> FlowDone + Send + 'static,
    ) {
        let post = self.poster();
        let _ = self.deps.jobs.submit(priority, JobLabel(label), move |_| {
            post(Internal::Flow(Box::new(f())));
        });
    }

    /// A job of this module finished.
    pub(crate) fn on_services_done(&mut self, done: ServicesDone) -> Result<()> {
        match done {
            ServicesDone::Trending {
                reply_to,
                with,
                answer,
            } => self.on_trending(reply_to, with, answer),
            ServicesDone::GifReady { chat, path } => return self.on_gif_ready(chat, path),
            ServicesDone::GifFailed => self.notice(Notice::ChatGifNotSent),
            ServicesDone::Checked { link, verdict } => self.on_link_checked(link, verdict)?,
        }
        Ok(())
    }
}
