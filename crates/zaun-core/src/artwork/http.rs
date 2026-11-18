//! The only HTTP client in the codebase (ADR-0018 §5, THREAT_MODEL T-18). Each client has a
//! fixed host allow-list: Steam artwork ([`ARTWORK_HOSTS`]), or the Chat services
//! ([`HttpClient::for_hosts`]: `crate::gifs`, `crate::linkcheck`).
//!
//! - HTTPS only, certificate validation by rustls with the webpki roots.
//! - First requests only to [`ARTWORK_REQUEST_HOSTS`]; redirects are followed by this module
//!   (never by the transport) and only to hosts of [`ARTWORK_REQUEST_HOSTS`] or
//!   [`ARTWORK_REDIRECT_HOSTS`], at most [`MAX_REDIRECTS`] times.
//! - [`TIMEOUT`] for the whole request including redirects, [`MAX_BODY`] response cap,
//!   at most [`MAX_CONCURRENT`] requests at once, `User-Agent: Zaun/<version>`, no
//!   cookies, proxies only from the standard environment variables.
//!
//! Callers gate every use on `artwork.online = "steam"` (see `steam`); this module does not
//! read preferences. Bodies are untrusted and go through `ImageCodec` or a size-capped JSON
//! parse. The network goes through [`Transport`] so tests can count, refuse or fake it.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

use crate::clock::Clock;
use crate::consts::{APP_NAME, APP_VERSION, ARTWORK_REDIRECT_HOSTS, ARTWORK_REQUEST_HOSTS};
use crate::error::{Error, ErrorCode};

/// Time budget of one request, redirects included.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Largest response body.
pub const MAX_BODY: u64 = 8 * 1024 * 1024;
/// Requests in flight at most.
pub const MAX_CONCURRENT: usize = 2;
/// Redirects followed at most.
pub const MAX_REDIRECTS: usize = 3;

/// `Zaun/<version>`.
pub fn user_agent() -> String {
    format!("{APP_NAME}/{APP_VERSION}")
}

/// Why a request produced no usable response.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HttpError {
    /// Not an `https://` URL.
    #[error("only https is allowed")]
    NotHttps,
    /// Not a URL this client understands (ports, user info and IP literals are refused).
    #[error("malformed url")]
    BadUrl,
    /// The host is not on the allow-list (for a first request or a redirect).
    #[error("host not allowed: {0}")]
    HostNotAllowed(String),
    /// More than [`MAX_REDIRECTS`] redirects, or a redirect without a location.
    #[error("too many or broken redirects")]
    Redirects,
    /// The body is larger than the cap.
    #[error("response too large")]
    TooLarge,
    /// The time budget ran out.
    #[error("timed out")]
    Timeout,
    /// Connection, TLS or protocol failure.
    #[error("transport error: {0}")]
    Transport(String),
}

impl From<HttpError> for Error {
    fn from(e: HttpError) -> Self {
        Error::new(ErrorCode::ArtUnreadable)
            .with("reason", "download")
            .with_source(e)
    }
}

/// A parsed `https://host/path?query` URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpsUrl {
    /// Lower-case host name.
    pub host: String,
    /// Path and query, starting with `/`.
    pub path: String,
}

impl HttpsUrl {
    /// Parse an absolute `https` URL.
    pub fn parse(s: &str) -> Result<HttpsUrl, HttpError> {
        let rest = match s.get(..8) {
            Some(p) if p.eq_ignore_ascii_case("https://") => &s[8..],
            _ => return Err(HttpError::NotHttps),
        };
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let host = &rest[..end];
        let mut path = rest[end..].split('#').next().unwrap_or("").to_owned();
        if path.is_empty() || path.starts_with('?') {
            path.insert(0, '/');
        }
        if host.is_empty()
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
            || host.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            || path.bytes().any(|b| b <= b' ' || b == 0x7F)
        {
            return Err(HttpError::BadUrl);
        }
        Ok(HttpsUrl {
            host: host.to_ascii_lowercase(),
            path,
        })
    }

    /// Resolve a `Location` header against this URL (absolute, `//host/…` or `/path`).
    pub fn join(&self, location: &str) -> Result<HttpsUrl, HttpError> {
        let l = location.trim();
        if l.len() >= 8 && l[..8].eq_ignore_ascii_case("https://") {
            HttpsUrl::parse(l)
        } else if let Some(rest) = l.strip_prefix("//") {
            HttpsUrl::parse(&format!("https://{rest}"))
        } else if l.starts_with('/') {
            HttpsUrl::parse(&format!("https://{}{l}", self.host))
        } else if l.contains("://") {
            Err(HttpError::NotHttps)
        } else {
            Err(HttpError::BadUrl)
        }
    }
}

impl std::fmt::Display for HttpsUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "https://{}{}", self.host, self.path)
    }
}

/// One hop, as the transport saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawResponse {
    /// HTTP status.
    pub status: u16,
    /// `Location`, for redirects.
    pub location: Option<String>,
    /// Body (at most the cap; empty for redirects).
    pub body: Vec<u8>,
}

/// Limits of one hop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HopLimits {
    /// Remaining time budget.
    pub timeout: Duration,
    /// Body cap.
    pub max_body: u64,
}

/// One GET without following redirects. Implementations enforce the limits.
pub trait Transport: Send + Sync {
    /// Perform the request.
    fn get(&self, url: &HttpsUrl, limits: HopLimits) -> Result<RawResponse, HttpError>;

    /// The same transport, but every request through the CONNECT proxy `proxy` (TLS stays
    /// end to end: the proxy sees only the host name). Transports without proxy support
    /// refuse.
    fn via(&self, proxy: &ProxyAddr) -> Result<Arc<dyn Transport>, HttpError> {
        let _ = proxy;
        Err(HttpError::Transport(
            "this transport has no proxy support".into(),
        ))
    }
}

/// An HTTP CONNECT proxy: `http://host:port` or `https://host:port`, with no user name,
/// password or path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyAddr {
    /// `https` (TLS to the proxy) rather than `http`.
    pub tls: bool,
    /// Lower-case host name.
    pub host: String,
    /// Port.
    pub port: u16,
}

impl ProxyAddr {
    /// Parse `http://host:port` or `https://host:port` (a trailing `/` is allowed; the port
    /// defaults to 80 or 443).
    pub fn parse(s: &str) -> Result<ProxyAddr, HttpError> {
        let s = s.trim();
        let lower = s.to_ascii_lowercase();
        let (tls, rest) = if let Some(r) = lower.strip_prefix("http://") {
            (false, r)
        } else if let Some(r) = lower.strip_prefix("https://") {
            (true, r)
        } else {
            return Err(HttpError::BadUrl);
        };
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        let (host, port) = match rest.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().map_err(|_| HttpError::BadUrl)?),
            None => (rest, if tls { 443 } else { 80 }),
        };
        if host.is_empty()
            || port == 0
            || !host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        {
            return Err(HttpError::BadUrl);
        }
        Ok(ProxyAddr {
            tls,
            host: host.to_owned(),
            port,
        })
    }
}

impl std::fmt::Display for ProxyAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let scheme = if self.tls { "https" } else { "http" };
        write!(f, "{scheme}://{}:{}", self.host, self.port)
    }
}

/// The production transport: `ureq` with rustls.
pub struct UreqTransport {
    agent: ureq::Agent,
    loopback: Option<SocketAddr>,
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqTransport {
    /// HTTPS only, no redirects, standard proxy variables, no cookies.
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(user_agent())
            .timeout_global(Some(TIMEOUT))
            .max_idle_connections(MAX_CONCURRENT)
            .build()
            .new_agent();
        Self {
            agent,
            loopback: None,
        }
    }

    /// Like [`UreqTransport::new`], but every request through the CONNECT proxy `proxy`
    /// (instead of the environment's proxy variables).
    pub fn via_proxy(proxy: &ProxyAddr) -> Result<Self, HttpError> {
        let proxy = ureq::Proxy::new(&proxy.to_string()).map_err(|_| HttpError::BadUrl)?;
        let agent = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(user_agent())
            .timeout_global(Some(TIMEOUT))
            .max_idle_connections(MAX_CONCURRENT)
            .proxy(Some(proxy))
            .build()
            .new_agent();
        Ok(Self {
            agent,
            loopback: None,
        })
    }

    /// **Tests only.** Sends every request as plain HTTP to `addr` (which must be a loopback
    /// address), with the original host as the first path segment: `GET /<host><path>`.
    /// Everything else (limits, redirects, status handling) is the production code path.
    #[doc(hidden)]
    pub fn plaintext_loopback(addr: SocketAddr) -> Self {
        assert!(addr.ip().is_loopback(), "loopback only");
        let agent = ureq::Agent::config_builder()
            .https_only(false)
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(user_agent())
            .proxy(None)
            .build()
            .new_agent();
        Self {
            agent,
            loopback: Some(addr),
        }
    }
}

fn map_ureq(e: ureq::Error) -> HttpError {
    match e {
        ureq::Error::Timeout(_) => HttpError::Timeout,
        ureq::Error::BodyExceedsLimit(_) => HttpError::TooLarge,
        ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => HttpError::Timeout,
        other => HttpError::Transport(other.to_string()),
    }
}

impl Transport for UreqTransport {
    fn via(&self, proxy: &ProxyAddr) -> Result<Arc<dyn Transport>, HttpError> {
        if self.loopback.is_some() {
            return Err(HttpError::Transport("no proxy in loopback tests".into()));
        }
        Ok(Arc::new(UreqTransport::via_proxy(proxy)?))
    }

    fn get(&self, url: &HttpsUrl, limits: HopLimits) -> Result<RawResponse, HttpError> {
        let target = match self.loopback {
            None => url.to_string(),
            Some(a) => format!("http://{a}/{}{}", url.host, url.path),
        };
        let mut resp = self
            .agent
            .get(&target)
            .config()
            .timeout_global(Some(limits.timeout))
            .build()
            .call()
            .map_err(map_ureq)?;
        let status = resp.status().as_u16();
        let header = |name: &str| {
            resp.headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let location = header("location");
        if (300..400).contains(&status) {
            return Ok(RawResponse {
                status,
                location,
                body: Vec::new(),
            });
        }
        if header("content-length")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .is_some_and(|n| n > limits.max_body)
        {
            return Err(HttpError::TooLarge);
        }
        let body = resp
            .body_mut()
            .with_config()
            .limit(limits.max_body)
            .read_to_vec()
            .map_err(map_ureq)?;
        Ok(RawResponse {
            status,
            location,
            body,
        })
    }
}

/// A final response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    /// The URL that answered (after redirects).
    pub url: HttpsUrl,
    /// HTTP status (not 3xx).
    pub status: u16,
    /// Body, at most the cap.
    pub body: Vec<u8>,
}

struct Gate {
    busy: Mutex<usize>,
    freed: Condvar,
}

struct Permit<'a>(&'a Gate);

impl Gate {
    fn acquire(&self) -> Permit<'_> {
        let mut busy = self.busy.lock();
        while *busy >= MAX_CONCURRENT {
            self.freed.wait(&mut busy);
        }
        *busy += 1;
        Permit(self)
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        *self.0.busy.lock() -= 1;
        self.0.freed.notify_one();
    }
}

/// The hosts one client may reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllowList {
    /// Hosts a first request may go to.
    pub request: &'static [&'static str],
    /// Hosts reachable only as redirect targets of an allowed request.
    pub redirect: &'static [&'static str],
}

/// Steam artwork (ADR-0018 §4): [`ARTWORK_REQUEST_HOSTS`] and [`ARTWORK_REDIRECT_HOSTS`].
pub const ARTWORK_HOSTS: AllowList = AllowList {
    request: &ARTWORK_REQUEST_HOSTS,
    redirect: &ARTWORK_REDIRECT_HOSTS,
};

/// The allow-listed client. Blocking: call it from a job (`Background` priority).
pub struct HttpClient {
    transport: Arc<dyn Transport>,
    clock: Arc<dyn Clock>,
    timeout: Duration,
    gate: Gate,
    hosts: AllowList,
}

#[cfg(test)]
fn allowed_first(host: &str) -> bool {
    ARTWORK_REQUEST_HOSTS.contains(&host)
}

#[cfg(test)]
fn allowed_redirect(host: &str) -> bool {
    allowed_first(host) || ARTWORK_REDIRECT_HOSTS.contains(&host)
}

impl HttpClient {
    /// A client over `transport` for the Steam artwork hosts ([`ARTWORK_HOSTS`]).
    pub fn new(transport: Arc<dyn Transport>, clock: Arc<dyn Clock>) -> Self {
        Self {
            transport,
            clock,
            timeout: TIMEOUT,
            gate: Gate {
                busy: Mutex::new(0),
                freed: Condvar::new(),
            },
            hosts: ARTWORK_HOSTS,
        }
    }

    /// A client over the same transport and clock that may reach only `hosts` (the Chat
    /// services, ADR-0033: GIF search and the link check). It has its own concurrency limit.
    #[must_use]
    pub fn for_hosts(&self, hosts: AllowList) -> Self {
        Self {
            transport: Arc::clone(&self.transport),
            clock: Arc::clone(&self.clock),
            timeout: self.timeout,
            gate: Gate {
                busy: Mutex::new(0),
                freed: Condvar::new(),
            },
            hosts,
        }
    }

    /// Like [`HttpClient::for_hosts`], but through the CONNECT proxy `proxy` (`None`:
    /// directly).
    pub fn for_hosts_via(
        &self,
        hosts: AllowList,
        proxy: Option<&ProxyAddr>,
    ) -> Result<Self, HttpError> {
        let mut c = self.for_hosts(hosts);
        if let Some(p) = proxy {
            c.transport = self.transport.via(p)?;
        }
        Ok(c)
    }

    /// The hosts this client may reach.
    pub fn hosts(&self) -> AllowList {
        self.hosts
    }

    /// Override the time budget (tests; never above [`TIMEOUT`]).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout.min(TIMEOUT);
        self
    }

    /// GET with the default body cap.
    pub fn get(&self, url: &str) -> Result<Response, HttpError> {
        self.get_capped(url, MAX_BODY)
    }

    /// GET with a body cap of `max_body` (at most [`MAX_BODY`]).
    pub fn get_capped(&self, url: &str, max_body: u64) -> Result<Response, HttpError> {
        let mut cur = HttpsUrl::parse(url)?;
        if !self.hosts.request.contains(&cur.host.as_str()) {
            return Err(HttpError::HostNotAllowed(cur.host));
        }
        let max_body = max_body.min(MAX_BODY);
        let _permit = self.gate.acquire();
        let start = self.clock.mono();
        for _ in 0..=MAX_REDIRECTS {
            let elapsed = self.clock.mono().saturating_duration_since(start);
            let Some(remaining) = self.timeout.checked_sub(elapsed).filter(|d| !d.is_zero()) else {
                return Err(HttpError::Timeout);
            };
            let raw = self.transport.get(
                &cur,
                HopLimits {
                    timeout: remaining,
                    max_body,
                },
            )?;
            if (300..400).contains(&raw.status) && raw.status != 304 {
                let loc = raw.location.as_deref().ok_or(HttpError::Redirects)?;
                let next = cur.join(loc)?;
                if !self.hosts.request.contains(&next.host.as_str())
                    && !self.hosts.redirect.contains(&next.host.as_str())
                {
                    tracing::warn!(host = %next.host, "redirect outside the allow-list refused");
                    return Err(HttpError::HostNotAllowed(next.host));
                }
                cur = next;
                continue;
            }
            if raw.body.len() as u64 > max_body {
                return Err(HttpError::TooLarge);
            }
            return Ok(Response {
                url: cur,
                status: raw.status,
                body: raw.body,
            });
        }
        Err(HttpError::Redirects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parsing() {
        let u =
            HttpsUrl::parse("https://CDN.cloudflare.steamstatic.com/steam/apps/70/x.jpg").unwrap();
        assert_eq!(u.host, "cdn.cloudflare.steamstatic.com");
        assert_eq!(u.path, "/steam/apps/70/x.jpg");
        assert_eq!(HttpsUrl::parse("https://h.example").unwrap().path, "/");
        assert_eq!(
            HttpsUrl::parse("https://h.example?q=1").unwrap().path,
            "/?q=1"
        );
        assert_eq!(
            HttpsUrl::parse("http://h.example/"),
            Err(HttpError::NotHttps)
        );
        assert_eq!(
            HttpsUrl::parse("ftp://h.example/"),
            Err(HttpError::NotHttps)
        );
        for bad in [
            "https://user@h.example/",
            "https://h.example:8443/",
            "https://127.0.0.1/",
            "https:///x",
            "https://h.example/a b",
        ] {
            assert_eq!(HttpsUrl::parse(bad), Err(HttpError::BadUrl), "{bad}");
        }
    }

    #[test]
    fn location_resolution() {
        let base = HttpsUrl::parse("https://a.example/x/y").unwrap();
        assert_eq!(base.join("/z").unwrap().to_string(), "https://a.example/z");
        assert_eq!(base.join("//b.example/z").unwrap().host, "b.example");
        assert_eq!(base.join("https://c.example/").unwrap().host, "c.example");
        assert_eq!(base.join("http://c.example/"), Err(HttpError::NotHttps));
        assert_eq!(base.join("z"), Err(HttpError::BadUrl));
    }

    #[test]
    fn allow_list() {
        assert!(allowed_first("cdn.cloudflare.steamstatic.com"));
        assert!(allowed_first("store.steampowered.com"));
        assert!(!allowed_first("shared.steamstatic.com"));
        assert!(allowed_redirect("shared.steamstatic.com"));
        assert!(!allowed_redirect("evil.example"));
        assert!(!allowed_redirect("steamstatic.com"));
    }

    #[test]
    fn proxy_addresses() {
        let p = ProxyAddr::parse("http://contentproxy.signal.org:443").unwrap();
        assert_eq!(
            (p.tls, p.host.as_str(), p.port),
            (false, "contentproxy.signal.org", 443)
        );
        assert_eq!(p.to_string(), "http://contentproxy.signal.org:443");
        assert_eq!(
            ProxyAddr::parse("HTTPS://Proxy.example/").unwrap().port,
            443
        );
        assert_eq!(ProxyAddr::parse("http://p.example").unwrap().port, 80);
        for bad in [
            "",
            "contentproxy.signal.org:443",
            "socks5://p.example:1080",
            "http://user:pw@p.example:8080",
            "http://p.example:0",
            "http://p.example:99999",
            "http://p.example:8080/path",
            "http://:8080",
        ] {
            assert_eq!(ProxyAddr::parse(bad), Err(HttpError::BadUrl), "{bad}");
        }
    }

    #[test]
    fn user_agent_names_the_version() {
        assert_eq!(user_agent(), format!("Zaun/{}", env!("CARGO_PKG_VERSION")));
    }
}
