//! GIFs for Chat from Giphy, through Signal's content proxy (ADR-0033, PRIVACY §3).
//!
//! - **What is sent.** Search sends only the typed words (`q`), the API key and fixed
//!   options (`limit`, `rating=pg-13`, `bundle=messaging_non_clips`). No user or install
//!   identifier, no language. Trending sends no words.
//! - **The route.** Every request, the pictures included, goes through an HTTP CONNECT
//!   proxy ([`DEFAULT_PROXY`], `chat.gif_proxy`; empty = directly). TLS runs end to end
//!   to Giphy: the proxy sees only the host names and Giphy sees only the proxy's address.
//!   Recorded on 2026-11-17: the proxy tunnels to `api.giphy.com`, `media*.giphy.com`,
//!   `i.giphy.com` and refuses other hosts (`example.com`).
//! - **The key.** `chat.gif_key`, by default [`DEFAULT_KEY`]: the public Giphy key Signal
//!   ships in its apps. The owner chose both knowing they are Signal's.
//! - **Hosts.** [`HOSTS`]: `api.giphy.com`, then only Giphy's media hosts. No redirects.
//!   Answers are capped ([`MAX_ANSWER`], [`MAX_PREVIEW`], [`MAX_SEND`]) and untrusted:
//!   [`parse_list`] keeps only GIF items whose addresses are on those hosts, [`decode`]
//!   limits sizes and frames.
//! - **Sending.** The sender downloads a GIF rendition of at most 1 MiB
//!   ([`send_rendition`]) and sends it as a file, so a friend's app never contacts Giphy
//!   or the proxy.
//! - **Attribution.** Giphy's terms (5A) require "Powered By GIPHY" where the API is used:
//!   the picker's footer shows it.

use std::sync::Arc;

use crate::artwork::http::{AllowList, HttpClient, HttpError, ProxyAddr};

/// Giphy's API host.
pub const API_HOST: &str = "api.giphy.com";
/// Giphy's media hosts (the renditions' addresses).
pub const MEDIA_HOSTS: [&str; 7] = [
    "media.giphy.com",
    "media0.giphy.com",
    "media1.giphy.com",
    "media2.giphy.com",
    "media3.giphy.com",
    "media4.giphy.com",
    "i.giphy.com",
];
/// The hosts the GIF client may reach (no redirects).
pub const HOSTS: AllowList = AllowList {
    request: &[
        API_HOST,
        MEDIA_HOSTS[0],
        MEDIA_HOSTS[1],
        MEDIA_HOSTS[2],
        MEDIA_HOSTS[3],
        MEDIA_HOSTS[4],
        MEDIA_HOSTS[5],
        MEDIA_HOSTS[6],
    ],
    redirect: &[],
};
/// `chat.gif_key`'s default: Signal's public Giphy key.
pub const DEFAULT_KEY: &str = crate::consts::GIF_DEFAULT_KEY;
/// `chat.gif_proxy`'s default: Signal's content proxy.
pub const DEFAULT_PROXY: &str = crate::consts::GIF_DEFAULT_PROXY;
/// Results per request.
pub const LIMIT: u32 = 24;
/// Content rating sent with every request.
pub const RATING: &str = "pg-13";
/// Giphy's rendition bundle for messaging apps (GIFs, no clips).
pub const BUNDLE: &str = "messaging_non_clips";
/// Longest search accepted (characters).
pub const MAX_QUERY: usize = 100;
/// Largest search or trending answer.
pub const MAX_ANSWER: u64 = 2 * 1024 * 1024;
/// Largest picture for the picker.
pub const MAX_PREVIEW: u64 = 1024 * 1024;
/// Largest GIF sent: what friends receive without asking (`chat::MAX_FILE`, 1 MiB).
pub const MAX_SEND: u64 = 1024 * 1024;

/// The key to use, from `chat.gif_key`: `None` when it is empty or not a plausible key
/// (letters and digits, 8 to 128 of them).
pub fn key(preference: &str) -> Option<String> {
    let k = preference.trim();
    ((8..=128).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| k.to_owned())
}

/// The proxy to use, from `chat.gif_proxy`: `Ok(None)` when it is empty (directly),
/// `Err` when it is not an `http://host:port` address.
pub fn proxy(preference: &str) -> Result<Option<ProxyAddr>, HttpError> {
    let p = preference.trim();
    if p.is_empty() {
        Ok(None)
    } else {
        ProxyAddr::parse(p).map(Some)
    }
}

/// Percent-encode `s` for a query value (RFC 3986 unreserved characters stay).
pub fn encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The search words as sent: trimmed, inner spaces collapsed, at most [`MAX_QUERY`]
/// characters. Empty means "trending".
pub fn clean_query(q: &str) -> String {
    q.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_QUERY)
        .collect()
}

fn list_url(key: &str, what: &str, query: Option<&str>) -> String {
    let mut url = format!("https://{API_HOST}/v1/gifs/{what}?api_key={key}");
    if let Some(q) = query {
        url.push_str("&q=");
        url.push_str(&encode_query(q));
    }
    url.push_str(&format!("&limit={LIMIT}&rating={RATING}&bundle={BUNDLE}"));
    url
}

/// The search address for `query`.
pub fn search_url(key: &str, query: &str) -> String {
    list_url(key, "search", Some(query))
}

/// The trending address.
pub fn trending_url(key: &str) -> String {
    list_url(key, "trending", None)
}

/// One GIF found.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Gif {
    /// Giphy's ID, or a stand-in's name in the preview.
    pub id: String,
    /// Its title, for the accessible name.
    pub title: String,
    /// The picker's picture (animated WebP or GIF); empty for a stand-in.
    pub preview_url: String,
    /// The preview's size in pixels (for the cell's aspect).
    pub size: [u16; 2],
    /// What is downloaded and sent (a GIF of at most [`MAX_SEND`]); empty for a stand-in
    /// and for a GIF with no rendition small enough to send.
    pub send_url: String,
}

impl Gif {
    /// A preview stand-in (no picture; the page draws a coloured card with the title).
    pub fn is_stand_in(&self) -> bool {
        self.preview_url.is_empty()
    }

    /// Giphy has a rendition small enough to send.
    pub fn sendable(&self) -> bool {
        !self.send_url.is_empty()
    }
}

/// A GIF chosen in the picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GifPick {
    /// One found on Giphy (downloaded, then sent).
    Found(Gif),
    /// A GIF already on this computer (a saved one from a conversation), sent as it is.
    /// Only files inside the Chat data folder are sent.
    Saved(std::path::PathBuf),
}

/// Why a list has no (more) results.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GifProblem {
    /// `chat.gif_key` is empty.
    NoKey,
    /// GIF search is turned off (`chat.gif_search`).
    Off,
    /// Giphy (or the relay) could not be reached.
    Offline,
    /// `chat.gif_proxy` is not an `http://host:port` address.
    BadRelay,
    /// Giphy refused the key (401 or 403).
    BadKey,
    /// Giphy refused for another reason (too many requests).
    Refused,
    /// The answer was not understood.
    BadAnswer,
}

/// The answer to `ChatCommand::GifSearch` and `GifTrending`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GifAnswer {
    /// The words searched (empty: trending).
    pub query: String,
    /// Results in Giphy's order.
    pub gifs: Vec<Gif>,
    /// Why there are none (or fewer), if something went wrong.
    pub problem: Option<GifProblem>,
}

impl GifAnswer {
    /// An answer with no results.
    pub fn problem(query: String, problem: GifProblem) -> Self {
        Self {
            query,
            gifs: Vec::new(),
            problem: Some(problem),
        }
    }
}

/// Downloaded picture bytes (Debug shows only the length).
#[derive(Clone, PartialEq, Eq)]
pub struct GifBytes(pub Arc<[u8]>);

impl std::fmt::Debug for GifBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GifBytes({} bytes)", self.0.len())
    }
}

/// The answer to `ChatCommand::GifPreview`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GifPreview {
    /// The address asked for.
    pub url: String,
    /// The picture, or `None` when it could not be fetched.
    pub bytes: Option<GifBytes>,
}

fn on_media_host(url: &str) -> bool {
    crate::artwork::http::HttpsUrl::parse(url).is_ok_and(|u| MEDIA_HOSTS.contains(&u.host.as_str()))
}

#[derive(Clone, Copy)]
struct Rendition<'a> {
    url: &'a str,
    size: [u16; 2],
    bytes: u64,
}

/// A number Giphy sends as a string ("598293") or a number.
fn number(v: Option<&serde_json::Value>) -> Option<u64> {
    let v = v?;
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// Rendition `name` of `images`: its `url` (a GIF) or, with `webp`, its `webp` address.
fn rendition<'a>(images: &'a serde_json::Value, name: &str, webp: bool) -> Option<Rendition<'a>> {
    let r = images.get(name)?;
    let (url_key, size_key) = if webp {
        ("webp", "webp_size")
    } else {
        ("url", "size")
    };
    let url = r.get(url_key)?.as_str()?;
    if !on_media_host(url) {
        return None;
    }
    let dim = |k: &str| number(r.get(k)).map_or(0, |v| u16::try_from(v).unwrap_or(u16::MAX));
    Some(Rendition {
        url,
        size: [dim("width"), dim("height")],
        bytes: number(r.get(size_key)).unwrap_or(u64::MAX),
    })
}

/// The picker's picture: the small animated WebP of the 200-wide rendition, else its GIF.
fn preview_rendition(images: &serde_json::Value) -> Option<Rendition<'_>> {
    [
        ("fixed_width_downsampled", true),
        ("fixed_width", true),
        ("fixed_width_downsampled", false),
        ("fixed_width_small", false),
        ("fixed_height_downsampled", false),
    ]
    .into_iter()
    .filter_map(|(n, w)| rendition(images, n, w))
    .find(|r| r.bytes <= MAX_PREVIEW)
}

/// What is sent: the largest GIF rendition of at most [`MAX_SEND`] (the original, else
/// the 200-high one, else smaller ones).
pub fn send_rendition(images: &serde_json::Value) -> Option<String> {
    [
        "original",
        "fixed_height",
        "fixed_width",
        "fixed_height_downsampled",
        "fixed_width_downsampled",
        "fixed_height_small",
        "fixed_width_small",
    ]
    .into_iter()
    .filter_map(|n| rendition(images, n, false))
    .find(|r| r.bytes <= MAX_SEND)
    .map(|r| r.url.to_owned())
}

/// Read a search or trending answer. Items that are not GIFs, or whose pictures are not
/// on Giphy's media hosts, are left out; the order is kept.
pub fn parse_list(body: &[u8]) -> Result<Vec<Gif>, GifProblem> {
    let v: serde_json::Value = serde_json::from_slice(body).map_err(|_| GifProblem::BadAnswer)?;
    match number(v.get("meta").and_then(|m| m.get("status"))) {
        Some(200) => {}
        Some(401 | 403) => return Err(GifProblem::BadKey),
        Some(_) => return Err(GifProblem::Refused),
        None => return Err(GifProblem::BadAnswer),
    }
    let items = v
        .get("data")
        .and_then(serde_json::Value::as_array)
        .ok_or(GifProblem::BadAnswer)?;
    let mut out = Vec::new();
    for it in items {
        if it.get("type").and_then(|t| t.as_str()).unwrap_or("gif") != "gif" {
            continue;
        }
        let Some(images) = it.get("images") else {
            continue;
        };
        // Kept even without a sendable rendition (choosing it says it is too large).
        let Some(preview) = preview_rendition(images) else {
            continue;
        };
        let send_url = send_rendition(images).unwrap_or_default();
        let text = |k: &str| it.get(k).and_then(|t| t.as_str()).unwrap_or("");
        let id: String = text("id")
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .take(64)
            .collect();
        if id.is_empty() {
            continue;
        }
        out.push(Gif {
            id,
            title: text("title").chars().take(120).collect(),
            preview_url: preview.url.to_owned(),
            size: preview.size,
            send_url,
        });
    }
    Ok(out)
}

fn list(http: &HttpClient, url: &str, query: String) -> GifAnswer {
    debug_assert_eq!(http.hosts(), HOSTS);
    match http.get_capped(url, MAX_ANSWER) {
        Err(e) => {
            // Never log the address: it carries the key.
            tracing::info!(error = %redact(&e), "gif list not fetched");
            GifAnswer::problem(query, GifProblem::Offline)
        }
        Ok(r) => match (r.status, parse_list(&r.body)) {
            (200, Ok(gifs)) => GifAnswer {
                query,
                gifs,
                problem: None,
            },
            (status, result) => {
                tracing::info!(status, "gif list refused");
                let p = match result {
                    Err(GifProblem::BadAnswer) | Ok(_) if matches!(status, 401 | 403) => {
                        GifProblem::BadKey
                    }
                    Err(GifProblem::BadAnswer) | Ok(_) if status != 200 => GifProblem::Refused,
                    Err(p) => p,
                    Ok(_) => GifProblem::BadAnswer,
                };
                GifAnswer::problem(query, p)
            }
        },
    }
}

fn redact(e: &HttpError) -> String {
    match e {
        HttpError::Transport(_) => "transport error".to_owned(),
        other => other.to_string(),
    }
}

/// Search Giphy for `query` (already cleaned, not empty). Blocking: call it from a job.
pub fn search(http: &HttpClient, key: &str, query: &str) -> GifAnswer {
    list(http, &search_url(key, query), query.to_owned())
}

/// Giphy's trending GIFs. Blocking: call it from a job.
pub fn trending(http: &HttpClient, key: &str) -> GifAnswer {
    list(http, &trending_url(key), String::new())
}

/// Fetch a picker picture (only from Giphy's media hosts). Blocking.
pub fn fetch_preview(http: &HttpClient, url: &str) -> Option<GifBytes> {
    if !on_media_host(url) {
        return None;
    }
    match http.get_capped(url, MAX_PREVIEW) {
        Ok(r) if r.status == 200 && !r.body.is_empty() => Some(GifBytes(r.body.into())),
        Ok(r) => {
            tracing::debug!(status = r.status, "gif preview not fetched");
            None
        }
        Err(e) => {
            tracing::debug!(error = %redact(&e), "gif preview not fetched");
            None
        }
    }
}

/// Why a GIF was not downloaded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DownloadError {
    /// Not one of Giphy's media addresses.
    #[error("not a Giphy media address")]
    NotGiphy,
    /// The request failed.
    #[error("download failed: {0}")]
    Network(HttpError),
    /// An HTTP status other than 200.
    #[error("the GIF service answered {0}")]
    Status(u16),
    /// The file is not a GIF.
    #[error("not a GIF")]
    NotGif,
}

/// Download the GIF to send (at most [`MAX_SEND`]); the bytes must be a GIF. Blocking.
pub fn download(http: &HttpClient, url: &str) -> Result<Vec<u8>, DownloadError> {
    if !on_media_host(url) {
        return Err(DownloadError::NotGiphy);
    }
    let r = http
        .get_capped(url, MAX_SEND)
        .map_err(DownloadError::Network)?;
    if r.status != 200 {
        return Err(DownloadError::Status(r.status));
    }
    if !(r.body.starts_with(b"GIF87a") || r.body.starts_with(b"GIF89a")) {
        return Err(DownloadError::NotGif);
    }
    Ok(r.body)
}

/// A file name for a downloaded GIF: the ID (safe characters only) and `n`.
pub fn outbox_name(id: &str, n: u64) -> String {
    let safe: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(60)
        .collect();
    let stem = if safe.is_empty() { "gif" } else { &safe };
    format!("{stem}-{n}.gif")
}

// ---- Decoding ----------------------------------------------------------------------------

/// One picture of an animation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Width and height in pixels.
    pub size: [usize; 2],
    /// RGBA, unpremultiplied.
    pub rgba: Vec<u8>,
    /// How long it shows, in milliseconds (at least 20; 0 or tiny delays become 100, as
    /// browsers do).
    pub delay_ms: u32,
}

/// Decoding limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeLimits {
    /// Largest width or height.
    pub max_side: u32,
    /// Most frames kept (the rest are dropped).
    pub max_frames: usize,
    /// Most decoded bytes over all frames (frames beyond are dropped).
    pub max_bytes: usize,
    /// Largest input.
    pub max_input: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_side: 1024,
            max_frames: 300,
            max_bytes: 64 * 1024 * 1024,
            max_input: 16 * 1024 * 1024,
        }
    }
}

/// Why a picture could not be decoded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// Larger than the limits.
    #[error("too large")]
    TooLarge,
    /// Not a GIF, WebP, PNG or JPEG the decoder reads.
    #[error("unreadable: {0}")]
    Unreadable(String),
}

/// Decode a GIF or animated WebP into frames (a still picture gives one frame), within
/// `limits`. Untrusted input: call it off the UI thread.
pub fn decode(bytes: &[u8], limits: DecodeLimits) -> Result<Vec<Frame>, DecodeError> {
    use image::AnimationDecoder;
    if bytes.len() > limits.max_input {
        return Err(DecodeError::TooLarge);
    }
    let bad = |e: image::ImageError| match e {
        image::ImageError::Limits(_) => DecodeError::TooLarge,
        other => DecodeError::Unreadable(other.to_string()),
    };
    let mut io_limits = image::Limits::default();
    io_limits.max_image_width = Some(limits.max_side);
    io_limits.max_image_height = Some(limits.max_side);
    io_limits.max_alloc = Some(limits.max_bytes as u64);
    let cursor = std::io::Cursor::new(bytes);
    let frames: Box<dyn Iterator<Item = image::ImageResult<image::Frame>>> =
        if bytes.starts_with(b"GIF8") {
            let mut d = image::codecs::gif::GifDecoder::new(cursor).map_err(bad)?;
            image::ImageDecoder::set_limits(&mut d, io_limits).map_err(bad)?;
            Box::new(d.into_frames())
        } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            let mut d = image::codecs::webp::WebPDecoder::new(cursor).map_err(bad)?;
            if !d.has_animation() {
                return still(bytes, limits);
            }
            image::ImageDecoder::set_limits(&mut d, io_limits).map_err(bad)?;
            Box::new(d.into_frames())
        } else {
            return still(bytes, limits);
        };
    let mut out = Vec::new();
    let mut total = 0usize;
    for f in frames {
        let f = f.map_err(bad)?;
        let (n, d) = f.delay().numer_denom_ms();
        let ms = n.checked_div(d).unwrap_or(0);
        let buf = f.into_buffer();
        let (w, h) = buf.dimensions();
        if w > limits.max_side || h > limits.max_side {
            return Err(DecodeError::TooLarge);
        }
        let len = buf.as_raw().len();
        if out.len() >= limits.max_frames || total + len > limits.max_bytes {
            break;
        }
        total += len;
        out.push(Frame {
            size: [w as usize, h as usize],
            rgba: buf.into_raw(),
            delay_ms: if ms < 20 { 100 } else { ms },
        });
    }
    if out.is_empty() {
        return Err(DecodeError::Unreadable("no frames".to_owned()));
    }
    Ok(out)
}

fn still(bytes: &[u8], limits: DecodeLimits) -> Result<Vec<Frame>, DecodeError> {
    let mut r = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| DecodeError::Unreadable(e.to_string()))?;
    let mut l = image::Limits::default();
    l.max_image_width = Some(limits.max_side);
    l.max_image_height = Some(limits.max_side);
    l.max_alloc = Some(limits.max_bytes as u64);
    r.limits(l);
    let img = r.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => DecodeError::TooLarge,
        other => DecodeError::Unreadable(other.to_string()),
    })?;
    let buf = img.into_rgba8();
    let (w, h) = buf.dimensions();
    Ok(vec![Frame {
        size: [w as usize, h as usize],
        rgba: buf.into_raw(),
        delay_ms: 100,
    }])
}

// ---- The preview ------------------------------------------------------------------------

/// The preview's stand-in GIFs (chat.html's twelve), as `(id, words)`.
pub const STAND_INS: [(&str, &str); 12] = [
    ("letsgo", "LET'S GO"),
    ("gg", "GG"),
    ("nope", "NOPE"),
    ("clutch", "CLUTCH"),
    ("oof", "OOF"),
    ("brb", "BRB"),
    ("nice", "NICE"),
    ("hype", "HYPE"),
    ("ez", "EZ"),
    ("rip", "RIP"),
    ("gn", "GOOD NIGHT"),
    ("ty", "THANK YOU"),
];

/// The words a stand-in shows (`None` for anything else).
pub fn stand_in_words(id: &str) -> Option<&'static str> {
    STAND_INS.iter().find(|(k, _)| *k == id).map(|(_, w)| *w)
}

/// The preview backend's answer, without any network: every stand-in for trending, the
/// ones whose words contain `query` for a search.
pub fn demo(query: &str) -> GifAnswer {
    let q = clean_query(query).to_lowercase();
    GifAnswer {
        gifs: STAND_INS
            .iter()
            .filter(|(_, w)| q.is_empty() || w.to_lowercase().contains(&q))
            .map(|(id, w)| Gif {
                id: (*id).to_owned(),
                title: (*w).to_owned(),
                preview_url: String::new(),
                size: [16, 10],
                send_url: String::new(),
            })
            .collect(),
        query: q,
        problem: None,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // Recorded on 2026-11-17 through contentproxy.signal.org with the default key
    // (`limit=4`): a search for "cat", trending, a search with no results, and a bad key
    // (HTTP 401).
    macro_rules! fixture {
        ($name:literal) => {
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/data/services/",
                $name
            ))
        };
    }
    const CAT: &[u8] = fixture!("giphy_search_cat.json");
    const TRENDING: &[u8] = fixture!("giphy_trending.json");
    const NONE: &[u8] = fixture!("giphy_search_none.json");
    const INVALID_KEY: &[u8] = fixture!("giphy_invalid_key.json");

    #[test]
    fn recorded_answers_keep_order_and_pick_renditions() {
        let gifs = parse_list(CAT).unwrap();
        let ids: Vec<&str> = gifs.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "774uK11mxE3W0cjE8L",
                "tphCApwvdtC1VJabZ1",
                "gSQp32H82WETR5EFO6",
                "GB0XWhoCDNNAW8M9vv"
            ]
        );
        let first = &gifs[0];
        assert_eq!(first.title, "High Five Ai GIF");
        // The picker shows the small animated WebP; the original (2.6 MB) is too large to
        // send, the 200-high GIF (0.6 MB) is sent.
        assert!(
            first.preview_url.ends_with("/200w_d.webp"),
            "{}",
            first.preview_url
        );
        assert_eq!(first.size, [200, 200]);
        assert!(first.send_url.ends_with("/200.gif"), "{}", first.send_url);
        assert!(gifs[1].send_url.ends_with("/200_d.gif"));
        assert!(
            gifs.iter()
                .all(|g| on_media_host(&g.preview_url) && g.sendable())
        );
        let t = parse_list(TRENDING).unwrap();
        assert_eq!(t.len(), 4);
        // A small original is sent as it is.
        assert!(t[0].send_url.ends_with("/giphy.gif"), "{}", t[0].send_url);
        assert_eq!(t[0].size, [200, 122]);
        assert_eq!(parse_list(NONE), Ok(vec![]));
    }

    #[test]
    fn a_refusal_is_not_an_empty_result() {
        assert_eq!(parse_list(INVALID_KEY), Err(GifProblem::BadKey));
        assert_eq!(
            parse_list(br#"{"data":[],"meta":{"status":429,"msg":"Too Many Requests"}}"#),
            Err(GifProblem::Refused)
        );
        assert_eq!(parse_list(b"<html>"), Err(GifProblem::BadAnswer));
        assert_eq!(
            parse_list(br#"{"data":{},"meta":{"status":200}}"#),
            Err(GifProblem::BadAnswer)
        );
        assert_eq!(parse_list(br#"{"data":[]}"#), Err(GifProblem::BadAnswer));
    }

    #[test]
    fn foreign_hosts_and_clips_are_left_out() {
        let body = br#"{"meta":{"status":200},"data":[
            {"type":"gif","id":"evil1","title":"x","images":{"fixed_width_downsampled":{"url":"https://evil.example/a.gif","size":"10","width":"200","height":"100"},"original":{"url":"https://evil.example/a.gif","size":"10"}}},
            {"type":"video","id":"clip1","title":"x","images":{"fixed_width_downsampled":{"url":"https://media.giphy.com/a.gif","size":"10"}}},
            {"type":"gif","id":"big1","title":"big","images":{"fixed_width_downsampled":{"webp":"https://media1.giphy.com/a.webp","webp_size":"10","width":"200","height":"100"},"original":{"url":"https://media1.giphy.com/o.gif","size":"5000000"}}}
        ]}"#;
        let gifs = parse_list(body).unwrap();
        assert_eq!(gifs.len(), 1);
        assert_eq!(gifs[0].id, "big1");
        assert!(!gifs[0].sendable());
    }

    #[test]
    fn only_the_words_the_key_and_fixed_options_are_sent() {
        let url = search_url("KEY123456", "good game & más");
        assert_eq!(
            url,
            "https://api.giphy.com/v1/gifs/search?api_key=KEY123456&q=good%20game%20%26%20m%C3%A1s&limit=24&rating=pg-13&bundle=messaging_non_clips"
        );
        let t = trending_url("KEY123456");
        assert_eq!(
            t,
            "https://api.giphy.com/v1/gifs/trending?api_key=KEY123456&limit=24&rating=pg-13&bundle=messaging_non_clips"
        );
    }

    #[test]
    fn queries_are_cleaned() {
        assert_eq!(clean_query("  good\t game \n"), "good game");
        assert_eq!(clean_query(&"x".repeat(300)).len(), MAX_QUERY);
    }

    #[test]
    fn keys_and_relays() {
        assert_eq!(key(DEFAULT_KEY).as_deref(), Some(DEFAULT_KEY));
        assert_eq!(key("  abcdEFGH123 ").as_deref(), Some("abcdEFGH123"));
        assert_eq!(key(""), None);
        assert_eq!(key("short"), None);
        assert_eq!(key("has/slash12345"), None);
        assert_eq!(key("key&q=x123456"), None);
        assert_eq!(proxy(""), Ok(None));
        assert_eq!(
            proxy(DEFAULT_PROXY).unwrap().unwrap().to_string(),
            "http://contentproxy.signal.org:443"
        );
        assert!(proxy("contentproxy.signal.org").is_err());
    }

    #[test]
    fn outbox_names_are_safe() {
        assert_eq!(outbox_name("hello-hi-662", 3), "hello-hi-662-3.gif");
        assert_eq!(outbox_name("../../x", 1), "x-1.gif");
        assert_eq!(outbox_name("", 1), "gif-1.gif");
    }

    #[test]
    fn previews_come_only_from_giphy() {
        assert!(on_media_host("https://media2.giphy.com/a.gif"));
        assert!(on_media_host("https://i.giphy.com/a.gif"));
        assert!(!on_media_host("https://api.giphy.com/a.gif"));
        assert!(!on_media_host("https://giphy.com/a.gif"));
        assert!(!on_media_host("http://media2.giphy.com/a.gif"));
        assert!(!on_media_host(
            "https://media2.giphy.com.evil.example/a.gif"
        ));
    }

    #[test]
    fn decodes_a_still_and_refuses_junk() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 3, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let f = decode(&png, DecodeLimits::default()).unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].size, [4, 3]);
        assert!(decode(b"GIF89a\x00", DecodeLimits::default()).is_err());
        assert!(decode(b"nothing", DecodeLimits::default()).is_err());
        let tight = DecodeLimits {
            max_side: 2,
            ..DecodeLimits::default()
        };
        assert_eq!(decode(&png, tight), Err(DecodeError::TooLarge));
    }

    /// A transport that records what was asked (with the proxy it went through) and
    /// answers from a table.
    #[derive(Default)]
    pub(crate) struct Fake {
        pub asked: Arc<std::sync::Mutex<Vec<String>>>,
        pub answers: Arc<Vec<(&'static str, u16, Vec<u8>)>>,
        pub proxy: Option<String>,
    }

    impl Fake {
        pub(crate) fn answering(answers: Vec<(&'static str, u16, Vec<u8>)>) -> Self {
            Self {
                answers: Arc::new(answers),
                ..Self::default()
            }
        }
    }

    impl crate::artwork::http::Transport for Fake {
        fn get(
            &self,
            url: &crate::artwork::http::HttpsUrl,
            _: crate::artwork::http::HopLimits,
        ) -> Result<crate::artwork::http::RawResponse, HttpError> {
            let u = url.to_string();
            let via = self.proxy.as_deref().unwrap_or("direct");
            self.asked.lock().unwrap().push(format!("{via} {u}"));
            let (_, status, body) = self
                .answers
                .iter()
                .find(|(prefix, _, _)| u.starts_with(prefix))
                .ok_or_else(|| HttpError::Transport("offline".into()))?;
            Ok(crate::artwork::http::RawResponse {
                status: *status,
                location: None,
                body: body.clone(),
            })
        }

        fn via(
            &self,
            proxy: &ProxyAddr,
        ) -> Result<Arc<dyn crate::artwork::http::Transport>, HttpError> {
            Ok(Arc::new(Fake {
                asked: Arc::clone(&self.asked),
                answers: Arc::clone(&self.answers),
                proxy: Some(proxy.to_string()),
            }))
        }
    }

    fn client(fake: Arc<Fake>, relay: &str) -> HttpClient {
        HttpClient::new(fake, Arc::new(crate::clock::SystemClock::new()))
            .for_hosts_via(HOSTS, proxy(relay).unwrap().as_ref())
            .unwrap()
    }

    #[test]
    fn only_the_words_and_the_key_go_out_and_only_through_the_relay() {
        let fake = Arc::new(Fake::answering(vec![
            ("https://api.giphy.com/", 200, CAT.to_vec()),
            ("https://media2.giphy.com/", 200, b"GIF89a....".to_vec()),
        ]));
        let http = client(Arc::clone(&fake), DEFAULT_PROXY);
        let a = search(&http, DEFAULT_KEY, "gg wp");
        assert_eq!(a.problem, None);
        assert_eq!(a.gifs.len(), 4);
        assert_eq!(
            download(&http, &a.gifs[0].send_url).map(|b| b.len()),
            Ok(10)
        );
        let asked = fake.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 2);
        assert_eq!(
            asked[0],
            format!(
                "http://contentproxy.signal.org:443 https://api.giphy.com/v1/gifs/search?api_key={DEFAULT_KEY}&q=gg%20wp&limit=24&rating=pg-13&bundle=messaging_non_clips"
            )
        );
        assert!(
            asked[1].starts_with("http://contentproxy.signal.org:443 https://media2.giphy.com/")
        );
        // Nothing else is reachable through this client.
        assert!(matches!(
            http.get("https://cdn.cloudflare.steamstatic.com/x.jpg"),
            Err(HttpError::HostNotAllowed(_))
        ));
        assert_eq!(
            download(&http, "https://evil.example/x.gif"),
            Err(DownloadError::NotGiphy)
        );
        assert_eq!(fetch_preview(&http, "https://api.giphy.com/x"), None);
        // An empty relay connects directly.
        let direct = Arc::new(Fake::answering(vec![(
            "https://api.giphy.com/",
            200,
            TRENDING.to_vec(),
        )]));
        let _ = trending(&client(Arc::clone(&direct), ""), DEFAULT_KEY);
        assert!(direct.asked.lock().unwrap()[0].starts_with("direct https://api.giphy.com/"));
    }

    #[test]
    fn failures_say_why() {
        let bad = Arc::new(Fake::answering(vec![(
            "https://api.giphy.com/",
            401,
            INVALID_KEY.to_vec(),
        )]));
        assert_eq!(
            trending(&client(bad, DEFAULT_PROXY), "BADKEY123").problem,
            Some(GifProblem::BadKey)
        );
        let busy = Arc::new(Fake::answering(vec![(
            "https://api.giphy.com/",
            429,
            b"slow down".to_vec(),
        )]));
        assert_eq!(
            trending(&client(busy, DEFAULT_PROXY), DEFAULT_KEY).problem,
            Some(GifProblem::Refused)
        );
        let offline = trending(
            &client(Arc::new(Fake::default()), DEFAULT_PROXY),
            DEFAULT_KEY,
        );
        assert_eq!(offline.problem, Some(GifProblem::Offline));
        let not_gif = Arc::new(Fake::answering(vec![(
            "https://media.giphy.com/",
            200,
            b"<html>".to_vec(),
        )]));
        assert_eq!(
            download(
                &client(not_gif, DEFAULT_PROXY),
                "https://media.giphy.com/a.gif"
            ),
            Err(DownloadError::NotGif)
        );
    }

    /// Live: search, a picture and a GIF through Signal's proxy with the real transport.
    /// Needs the network: `cargo test -p easyplay-core --lib gifs::tests::live -- --ignored`.
    #[test]
    #[ignore = "reaches Giphy through contentproxy.signal.org"]
    fn live_search_preview_and_download_through_the_relay() {
        let base = HttpClient::new(
            Arc::new(crate::artwork::http::UreqTransport::new()),
            Arc::new(crate::clock::SystemClock::new()),
        );
        let http = base
            .for_hosts_via(HOSTS, proxy(DEFAULT_PROXY).unwrap().as_ref())
            .unwrap();
        let a = search(&http, DEFAULT_KEY, "cat");
        assert_eq!(a.problem, None);
        assert!(!a.gifs.is_empty());
        let g = a.gifs.iter().find(|g| g.sendable()).unwrap();
        let preview = fetch_preview(&http, &g.preview_url).unwrap();
        assert!(
            !decode(&preview.0, DecodeLimits::default())
                .unwrap()
                .is_empty()
        );
        let gif = download(&http, &g.send_url).unwrap();
        assert!(gif.len() as u64 <= MAX_SEND);
        assert!(decode(&gif, DecodeLimits::default()).unwrap().len() > 1);
        assert_eq!(trending(&http, DEFAULT_KEY).problem, None);
        // The relay is really used: a closed one means no answer.
        let closed = base
            .for_hosts_via(HOSTS, proxy("http://localhost:9").unwrap().as_ref())
            .unwrap();
        assert_eq!(
            trending(&closed, DEFAULT_KEY).problem,
            Some(GifProblem::Offline)
        );
    }

    #[test]
    fn demo_answers_without_network() {
        assert_eq!(demo("").gifs.len(), 12);
        let a = demo("  Good ");
        assert_eq!(a.gifs.len(), 1);
        assert_eq!(a.gifs[0].id, "gn");
        assert!(a.gifs[0].is_stand_in());
        assert_eq!(stand_in_words("gg"), Some("GG"));
    }
}
