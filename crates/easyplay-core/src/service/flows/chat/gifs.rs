//! GIF search, trending and sending (ADR-0033).
//!
//! - Trending is kept for [`TRENDING_FOR`], so opening the picker again sends nothing.
//! - A chosen Giphy GIF is downloaded into `<chat>/gif-downloads/` and handed to
//!   `ChatCommand::SendFile`, which copies it at once into the conversation's files; the
//!   download is then deleted, so no copy of Giphy's media is kept beyond the sent file. A
//!   saved GIF is sent from where it is, if it is inside the Chat folder.
use super::services::ServicesDone;
use crate::artwork::http::HttpClient;
use crate::gifs::{self, GifAnswer, GifPick, GifProblem};
use crate::service::prelude::*;

/// Where a chosen Giphy GIF waits until `SendFile` has copied it (`<chat>/gif-downloads`).
pub const DOWNLOAD_DIR: &str = "gif-downloads";
/// How long Giphy's trending list is reused (for the same key and relay).
pub const TRENDING_FOR: Duration = Duration::from_secs(10 * 60);

impl AppService {
    pub(super) fn gif_search(&mut self, query: &str, reply_to: QueryId) {
        let query = gifs::clean_query(query);
        if query.is_empty() {
            return self.gif_trending(reply_to);
        }
        let (http, key) = match self.gif_access() {
            Ok(a) => a,
            Err(p) => return self.answer_gifs(reply_to, GifAnswer::problem(query, p)),
        };
        self.answer_on_pool("chat-gif-search", reply_to, move |_| {
            QueryResult::Gifs(gifs::search(&http, &key, &query))
        });
    }

    pub(super) fn gif_trending(&mut self, reply_to: QueryId) {
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
            Err(p) => return self.answer_gifs(reply_to, GifAnswer::problem(String::new(), p)),
        };
        self.services_job("chat-gif-trending", move || ServicesDone::Trending {
            reply_to,
            with,
            answer: gifs::trending(&http, &key),
        });
    }

    pub(super) fn gif_preview(&mut self, url: String, reply_to: QueryId) {
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

    pub(super) fn send_gif(&mut self, chat: ChatRef, gif: GifPick) -> Result<()> {
        match gif {
            GifPick::Saved(path) => self.send_saved_gif(chat, path),
            GifPick::Found(g) => {
                self.download_and_send_gif(chat, &g);
                Ok(())
            }
        }
    }

    fn send_saved_gif(&mut self, chat: ChatRef, path: PathBuf) -> Result<()> {
        let fs = &self.deps.fs;
        let inside = fs
            .canonicalize(&path)
            .ok()
            .zip(fs.canonicalize(&self.deps.paths.chat_dir).ok())
            .is_some_and(|(p, d)| p.starts_with(d));
        if inside && fs.metadata_follow(&path).is_ok_and(|m| m.is_file()) {
            return self.chat_send_gif_file(chat, path);
        }
        tracing::info!("saved gif not sent: not a file in the chat folder");
        self.notice(Notice::ChatGifNotSent);
        Ok(())
    }

    fn download_and_send_gif(&mut self, chat: ChatRef, g: &gifs::Gif) {
        // The picker shows Giphy's GIFs only while search is allowed.
        let http = match self.gif_access() {
            Ok((h, _)) if g.sendable() => h,
            _ => return self.notice(Notice::ChatGifNotSent),
        };
        self.flow.services.sent += 1;
        let name = gifs::outbox_name(&g.id, self.flow.services.sent);
        let send_url = g.send_url.clone();
        let fs = Arc::clone(&self.deps.fs);
        let dir = self.deps.paths.chat_dir.join(DOWNLOAD_DIR);
        self.services_job("chat-gif-send", move || {
            let bytes = match gifs::download(&http, &send_url) {
                Ok(b) => b,
                Err(e) => {
                    tracing::info!(error = %e, "gif not downloaded");
                    return ServicesDone::GifFailed;
                }
            };
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

    pub(super) fn on_trending(&mut self, reply_to: QueryId, with: String, answer: GifAnswer) {
        if answer.problem.is_none() {
            let now = self.deps.clock.mono();
            self.flow.services.trending = Some((now, with, answer.clone()));
        }
        self.answer_gifs(reply_to, answer);
    }

    pub(super) fn on_gif_ready(&mut self, chat: ChatRef, path: PathBuf) -> Result<()> {
        // `SendFile` copies the file before it returns; the download goes.
        let sent = self.chat_send_gif_file(chat, path.clone());
        if let Err(e) = self.deps.fs.remove_file(&path) {
            tracing::info!(error = %e, "downloaded gif not removed");
        }
        sent
    }

    fn chat_send_gif_file(&mut self, chat: ChatRef, path: PathBuf) -> Result<()> {
        self.chat_command(ChatCommand::SendFile {
            chat,
            path,
            text: String::new(),
        })
    }

    fn answer_gifs(&self, id: QueryId, answer: GifAnswer) {
        self.emit(Event::QueryResult {
            id,
            result: QueryResult::Gifs(answer),
        });
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

    /// The GIF client and key, or why search is unavailable.
    fn gif_access(&mut self) -> Result<(Arc<HttpClient>, String), GifProblem> {
        if !self.prefs.chat.gif_search {
            return Err(GifProblem::Off);
        }
        let key = gifs::key(&self.prefs.chat.gif_key).ok_or(GifProblem::NoKey)?;
        let http = self.gif_http()?;
        Ok((http, key))
    }
}
