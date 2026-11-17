//! Chat's web services (ADR-0033): GIF search from Giphy ([`crate::gifs`]) and opening a
//! message's link, checked first when `chat.check_links` is on ([`crate::linkcheck`]).
//!
//! - Every request runs on the job pool through the allow-listed HTTP client, derived from
//!   the artwork client with the service's own host list. Without that client (tests, a
//!   build without network) GIF search answers "unavailable" and links open unchecked
//!   only when the check is off.
//! - Trending is kept for [`TRENDING_FOR`], so opening the picker again sends nothing.
//! - A chosen Giphy GIF is downloaded into `<chat>/gif-downloads/` and handed to
//!   `ChatCommand::SendFile`, which copies it at once into the conversation's files; the
//!   download is then deleted, so no copy of Giphy's media is kept beyond the sent file. A
//!   saved GIF is sent from where it is, if it is inside the Chat folder.
//! - Picker pictures are `Background` jobs, so they never hold up the user's own actions.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use super::super::app_service::{AppService, Internal};
use super::super::command::ChatCommand;
use super::super::event::{Event, Notice, QueryResult};
use super::FlowDone;
use crate::artwork::http::HttpClient;
use crate::clock::MonoInstant;
use crate::gifs::{self, GifAnswer, GifPick, GifProblem};
use crate::jobs::{JobLabel, JobPriority};
use crate::linkcheck::{self, CheckError, LinkStep, Verdict, WebLink};
use crate::model::ChatRef;

/// Where a chosen Giphy GIF waits until `SendFile` has copied it (`<chat>/gif-downloads`).
pub const DOWNLOAD_DIR: &str = "gif-downloads";
/// How long Giphy's trending list is reused (for the same key and relay).
pub const TRENDING_FOR: Duration = Duration::from_secs(10 * 60);

/// Whether `c` is one of this module's commands.
pub(crate) fn handles(c: &ChatCommand) -> bool {
    matches!(
        c,
        ChatCommand::OpenLink(_)
            | ChatCommand::OpenLinkAnyway(_)
            | ChatCommand::GifSearch { .. }
            | ChatCommand::GifTrending { .. }
            | ChatCommand::GifPreview { .. }
            | ChatCommand::SendGif { .. }
    )
}

/// What this module keeps between messages.
#[derive(Default)]
pub(crate) struct ServicesState {
    /// The GIF client and the `chat.gif_proxy` it was made for.
    gif_http: Option<(String, Arc<HttpClient>)>,
    link_http: Option<Arc<HttpClient>>,
    /// Trending, when, and for which key and relay.
    trending: Option<(MonoInstant, String, GifAnswer)>,
    sent: u64,
}

/// Completions of this module's jobs.
pub(crate) enum ServicesDone {
    /// Trending arrived (kept, then answered).
    Trending {
        reply_to: crate::ids::QueryId,
        /// The key and relay it was asked with.
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
    /// The link check's client (directly, never through the GIF relay).
    fn link_http(&mut self) -> Option<Arc<HttpClient>> {
        let base = self.deps.http.as_ref()?;
        Some(Arc::clone(self.flow.services.link_http.get_or_insert_with(
            || Arc::new(base.for_hosts(linkcheck::HOSTS)),
        )))
    }

    /// The key and relay as one string (what trending was fetched with).
    fn gif_settings(&self) -> String {
        format!(
            "{}\n{}",
            self.prefs.chat.gif_key.trim(),
            self.prefs.chat.gif_proxy.trim()
        )
    }

    /// The GIF client, through `chat.gif_proxy` (made again when it changes).
    fn gif_http(&mut self) -> Result<Arc<HttpClient>, GifProblem> {
        let relay = self.prefs.chat.gif_proxy.trim().to_owned();
        if let Some((r, c)) = &self.flow.services.gif_http
            && *r == relay
        {
            return Ok(Arc::clone(c));
        }
        let base = self.deps.http.as_ref().ok_or(GifProblem::Offline)?;
        let proxy = gifs::proxy(&relay).map_err(|_| GifProblem::BadRelay)?;
        let client = Arc::new(
            base.for_hosts_via(gifs::HOSTS, proxy.as_ref())
                .map_err(|_| GifProblem::BadRelay)?,
        );
        self.flow.services.gif_http = Some((relay, Arc::clone(&client)));
        Ok(client)
    }

    fn services_job(&self, label: &'static str, f: impl FnOnce() -> ServicesDone + Send + 'static) {
        self.pool(label, JobPriority::Interactive, move || {
            FlowDone::ChatServices(f())
        });
    }

    fn pool(
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

    /// The GIF client and key, or why search is unavailable.
    fn gif_access(&mut self) -> Result<(Arc<HttpClient>, String), GifProblem> {
        if !self.prefs.chat.gif_search {
            return Err(GifProblem::Off);
        }
        let key = gifs::key(&self.prefs.chat.gif_key).ok_or(GifProblem::NoKey)?;
        let http = self.gif_http()?;
        Ok((http, key))
    }

    /// One of [`handles`]'s commands.
    pub(crate) fn chat_service_command(&mut self, c: ChatCommand) {
        match c {
            ChatCommand::OpenLink(url) => self.open_chat_link(&url, true),
            ChatCommand::OpenLinkAnyway(url) => self.open_chat_link(&url, false),
            ChatCommand::GifSearch { query, reply_to } => {
                let query = gifs::clean_query(&query);
                if query.is_empty() {
                    return self.chat_service_command(ChatCommand::GifTrending { reply_to });
                }
                let (http, key) = match self.gif_access() {
                    Ok(a) => a,
                    Err(p) => return self.answer_gifs(reply_to, GifAnswer::problem(query, p)),
                };
                self.answer_on_pool("chat-gif-search", reply_to, move |_| {
                    QueryResult::Gifs(gifs::search(&http, &key, &query))
                });
            }
            ChatCommand::GifTrending { reply_to } => {
                let now = self.deps.clock.mono();
                let with = self.gif_settings();
                if let Some((at, w, answer)) = &self.flow.services.trending
                    && *w == with
                    && now.saturating_duration_since(*at) < TRENDING_FOR
                {
                    let answer = answer.clone();
                    return self.answer_gifs(reply_to, answer);
                }
                let (http, key) = match self.gif_access() {
                    Ok(a) => a,
                    Err(p) => {
                        return self.answer_gifs(reply_to, GifAnswer::problem(String::new(), p));
                    }
                };
                self.services_job("chat-gif-trending", move || ServicesDone::Trending {
                    reply_to,
                    with,
                    answer: gifs::trending(&http, &key),
                });
            }
            ChatCommand::GifPreview { url, reply_to } => {
                let http = if self.prefs.chat.gif_search {
                    self.gif_http().ok()
                } else {
                    None
                };
                self.pool("chat-gif-preview", JobPriority::Background, move || {
                    let bytes = http.and_then(|h| gifs::fetch_preview(&h, &url));
                    FlowDone::Query {
                        id: reply_to,
                        result: QueryResult::GifPreview(gifs::GifPreview { url, bytes }),
                    }
                });
            }
            ChatCommand::SendGif { chat, gif } => self.send_gif(chat, gif),
            other => tracing::debug!(?other, "not a chat service command"),
        }
    }

    fn answer_gifs(&self, id: crate::ids::QueryId, answer: GifAnswer) {
        self.emit(Event::QueryResult {
            id,
            result: QueryResult::Gifs(answer),
        });
    }

    fn send_gif(&mut self, chat: ChatRef, gif: GifPick) {
        let chat_dir = self.deps.paths.chat_dir.clone();
        let fs = Arc::clone(&self.deps.fs);
        match gif {
            GifPick::Saved(path) => {
                let inside = fs
                    .canonicalize(&path)
                    .ok()
                    .zip(fs.canonicalize(&chat_dir).ok())
                    .is_some_and(|(p, d)| p.starts_with(d));
                if inside && fs.metadata_follow(&path).is_ok_and(|m| m.is_file()) {
                    self.chat_command(ChatCommand::SendFile {
                        chat,
                        path,
                        text: String::new(),
                    });
                } else {
                    tracing::info!("saved gif not sent: not a file in the chat folder");
                    self.notice(Notice::ChatGifNotSent);
                }
            }
            GifPick::Found(g) => {
                // The picker shows Giphy's GIFs only while search is allowed.
                let http = match self.gif_access() {
                    Ok((h, _)) if g.sendable() => h,
                    _ => return self.notice(Notice::ChatGifNotSent),
                };
                let st = &mut self.flow.services;
                st.sent += 1;
                let name = gifs::outbox_name(&g.id, st.sent);
                self.services_job("chat-gif-send", move || {
                    let bytes = match gifs::download(&http, &g.send_url) {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::info!(error = %e, "gif not downloaded");
                            return ServicesDone::GifFailed;
                        }
                    };
                    let dir = chat_dir.join(DOWNLOAD_DIR);
                    let path = dir.join(name);
                    let written = fs
                        .create_dir_all(&dir, 0o700)
                        .and_then(|()| fs.atomic_write(&path, &bytes, 0o600));
                    match written {
                        Ok(()) => ServicesDone::GifReady { chat, path },
                        Err(e) => {
                            tracing::info!(error = %e, "gif not written to the outbox");
                            ServicesDone::GifFailed
                        }
                    }
                });
            }
        }
    }

    fn open_chat_link(&mut self, url: &str, check: bool) {
        let Some(link) = WebLink::parse(url) else {
            tracing::info!("chat link not opened: not an http or https link");
            return;
        };
        let http = match linkcheck::step(check && self.prefs.chat.check_links, &link) {
            LinkStep::Open => return self.open_web_link(link),
            LinkStep::Check => self.link_http(),
            LinkStep::Ask => None,
        };
        let Some(http) = http else {
            return self.ask_unchecked(&link);
        };
        self.services_job("chat-link-check", move || {
            let verdict = linkcheck::check(&http, &link);
            ServicesDone::Checked { link, verdict }
        });
    }

    fn ask_unchecked(&self, link: &WebLink) {
        self.notice(Notice::ChatUnsafeLink {
            url: link.url().to_owned(),
            host: link.host().to_owned(),
            unchecked: true,
        });
    }

    fn open_web_link(&self, link: WebLink) {
        let Some(d) = self.deps.desktop.clone() else {
            return self.fail(crate::error::Error::not_available("DESKTOP-005"));
        };
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("chat-open-link"),
            move |_| {
                if let Err(e) = d.opener.open_web(&link) {
                    post(Internal::Desktop(Box::new(
                        super::desktop::DesktopDone::Failed(e),
                    )));
                }
            },
        );
    }

    /// A job of this module finished.
    pub(crate) fn on_services_done(&mut self, done: ServicesDone) {
        match done {
            ServicesDone::Trending {
                reply_to,
                with,
                answer,
            } => {
                if answer.problem.is_none() {
                    let now = self.deps.clock.mono();
                    self.flow.services.trending = Some((now, with, answer.clone()));
                }
                self.answer_gifs(reply_to, answer);
            }
            ServicesDone::GifReady { chat, path } => {
                // `SendFile` copies the file before it returns; the download goes.
                self.chat_command(ChatCommand::SendFile {
                    chat,
                    path: path.clone(),
                    text: String::new(),
                });
                if let Err(e) = self.deps.fs.remove_file(&path) {
                    tracing::info!(error = %e, "downloaded gif not removed");
                }
            }
            ServicesDone::GifFailed => self.notice(Notice::ChatGifNotSent),
            ServicesDone::Checked { link, verdict } => match verdict {
                Ok(Verdict::Safe | Verdict::Unknown) => self.open_web_link(link),
                Ok(Verdict::Unsafe) => self.notice(Notice::ChatUnsafeLink {
                    url: link.url().to_owned(),
                    host: link.host().to_owned(),
                    unchecked: false,
                }),
                Err(e) => {
                    tracing::info!(error = %e, "link check gave no answer");
                    self.ask_unchecked(&link);
                }
            },
        }
    }
}
