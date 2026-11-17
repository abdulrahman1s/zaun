//! Opening links from Chat messages, and the optional link check (ADR-0033; PRIVACY §3).
//!
//! - [`WebLink::parse`] accepts only `http://` and `https://` links without control
//!   characters or backslashes, and works out the host a browser would contact (after any
//!   `user@`, without the port, lower case, no trailing dot).
//! - With `chat.check_links` on (off by default) the host, and nothing else, is sent to
//!   Google's Safe Browsing status page ([`SAFE_BROWSING_HOST`]) before the link opens.
//!   IP addresses, `localhost` and one-label names are never sent ([`WebLink::checkable`]).
//! - The answer starts with the anti-JSON prefix `)]}'` and is an array
//!   `[["sb.ssr", status, flag × 5, time, host, …]]`. Recorded on 2026-11-17
//!   (`tests/data/services/sb_*.txt`): status 1 with every flag false for example.com;
//!   status 3 with flags for Google's test sites; 4 for github.com (a site with content
//!   from many users); 6 for a name Google has no data on. [`parse_status`] maps any flag
//!   or status 2 or 3 to [`Verdict::Unsafe`], status 1 without flags to [`Verdict::Safe`],
//!   and every other status to [`Verdict::Unknown`]. Anything else is [`CheckError::Format`].

use crate::artwork::http::{AllowList, HttpClient, HttpError};

/// Google's transparency-report host that answers the Safe Browsing status.
pub const SAFE_BROWSING_HOST: &str = "transparencyreport.google.com";
/// The hosts the link check may reach (no redirects).
pub const HOSTS: AllowList = AllowList {
    request: &[SAFE_BROWSING_HOST],
    redirect: &[],
};
/// Largest status answer read.
pub const MAX_ANSWER: u64 = 64 * 1024;
/// Longest link accepted.
pub const MAX_LINK: usize = 4096;

/// A link that may be handed to the browser: `http` or `https`, with its host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebLink {
    url: String,
    host: String,
}

impl WebLink {
    /// Check `s` (trimmed). `None` for other schemes, control characters, spaces,
    /// backslashes, an empty host or an over-long link.
    pub fn parse(s: &str) -> Option<WebLink> {
        let url = s.trim();
        if url.len() > MAX_LINK
            || url
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
        {
            return None;
        }
        let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
        let rest = if lower.starts_with("https://") {
            &url[8..]
        } else if lower.starts_with("http://") {
            &url[7..]
        } else {
            return None;
        };
        let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
        // Browsers go to what follows the last `@`.
        let hostport = authority.rsplit('@').next().unwrap_or(authority);
        let host = if let Some(v6) = hostport.strip_prefix('[') {
            // An IPv6 literal: kept with its brackets.
            let end = v6.find(']')?;
            let after = &v6[end + 1..];
            if !(after.is_empty() || port_ok(after)) {
                return None;
            }
            format!("[{}]", v6[..end].to_ascii_lowercase())
        } else {
            let (h, port) = match hostport.rfind(':') {
                Some(i) => (&hostport[..i], &hostport[i..]),
                None => (hostport, ""),
            };
            if !(port.is_empty() || port_ok(port)) {
                return None;
            }
            h.trim_end_matches('.').to_lowercase()
        };
        if host.is_empty() || host.starts_with('.') || host.contains("..") {
            return None;
        }
        Some(WebLink {
            url: url.to_owned(),
            host,
        })
    }

    /// The link as written.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The host a browser contacts.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Whether the host may be sent to the link check: a public DNS name in ASCII with at
    /// least two labels. IP addresses, `localhost`, one-label (local network) names and
    /// names with other characters are never sent.
    pub fn checkable(&self) -> bool {
        let h = self.host.as_str();
        let ascii = h
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.');
        let ipv4 = h.bytes().all(|b| b.is_ascii_digit() || b == b'.');
        let local = h == "localhost" || h.ends_with(".localhost") || h.ends_with(".local");
        ascii && !ipv4 && !local && h.contains('.') && !h.starts_with('-')
    }
}

impl WebLink {
    /// A link to this computer or the local network: an IP address, `localhost`, a
    /// one-label or `.local` name. Google knows nothing of these, so they are not checked.
    pub fn local(&self) -> bool {
        let h = self.host.as_str();
        h.starts_with('[')
            || h.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            || h == "localhost"
            || h.ends_with(".localhost")
            || h.ends_with(".local")
            || !h.contains('.')
    }
}

/// What opening a link does first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkStep {
    /// Open it in the browser now.
    Open,
    /// Ask Google Safe Browsing about its host, then open or warn.
    Check,
    /// Ask the user: the host cannot be checked (a name in other characters, which a
    /// look-alike site may use).
    Ask,
}

/// The step for `link` when the check is on (`check`) or off. Local hosts are opened
/// without a check; a public host that cannot be sent is not opened silently.
pub fn step(check: bool, link: &WebLink) -> LinkStep {
    if !check || link.local() {
        LinkStep::Open
    } else if link.checkable() {
        LinkStep::Check
    } else {
        LinkStep::Ask
    }
}

fn port_ok(p: &str) -> bool {
    p.strip_prefix(':')
        .is_some_and(|d| !d.is_empty() && d.len() <= 5 && d.bytes().all(|b| b.is_ascii_digit()))
}

/// What the link check says about a site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// No unsafe content found.
    Safe,
    /// Google Safe Browsing flags the site (or some of its pages).
    Unsafe,
    /// Google has no verdict for the whole site (no data, or content from many users).
    Unknown,
}

/// Why the check gave no verdict.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CheckError {
    /// The request failed (offline, timeout, refused).
    #[error("the link check could not be reached: {0}")]
    Network(HttpError),
    /// An HTTP status other than 200.
    #[error("the link check answered {0}")]
    Status(u16),
    /// The answer is not in the expected form (the format may have changed).
    #[error("the link check's answer was not understood")]
    Format,
}

/// The status page's address for `host`.
pub fn status_url(host: &str) -> String {
    format!(
        "https://{SAFE_BROWSING_HOST}/transparencyreport/api/v3/safebrowsing/status?site={}",
        crate::gifs::encode_query(host)
    )
}

/// Read a status answer for `host`.
pub fn parse_status(body: &[u8], host: &str) -> Result<Verdict, CheckError> {
    let text = std::str::from_utf8(body).map_err(|_| CheckError::Format)?;
    let text = text.trim_start();
    let json = text.strip_prefix(")]}'").unwrap_or(text);
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| CheckError::Format)?;
    let row = v
        .as_array()
        .and_then(|a| a.first())
        .and_then(|r| r.as_array())
        .ok_or(CheckError::Format)?;
    if row.first().and_then(|t| t.as_str()) != Some("sb.ssr") {
        return Err(CheckError::Format);
    }
    let status = row
        .get(1)
        .and_then(serde_json::Value::as_u64)
        .ok_or(CheckError::Format)?;
    let mut flagged = false;
    for i in 2..=6 {
        flagged |= row
            .get(i)
            .and_then(serde_json::Value::as_bool)
            .ok_or(CheckError::Format)?;
    }
    if let Some(echo) = row.get(8).and_then(|h| h.as_str())
        && !echo.trim_end_matches('.').eq_ignore_ascii_case(host)
    {
        return Err(CheckError::Format);
    }
    Ok(match status {
        _ if flagged => Verdict::Unsafe,
        2 | 3 => Verdict::Unsafe,
        1 => Verdict::Safe,
        _ => Verdict::Unknown,
    })
}

/// Ask Google Safe Browsing about `link`'s host. Blocking: call it from a job. Only the
/// host is sent; the caller checks [`WebLink::checkable`] first.
pub fn check(http: &HttpClient, link: &WebLink) -> Result<Verdict, CheckError> {
    debug_assert_eq!(http.hosts(), HOSTS);
    let resp = http
        .get_capped(&status_url(link.host()), MAX_ANSWER)
        .map_err(CheckError::Network)?;
    if resp.status != 200 {
        return Err(CheckError::Status(resp.status));
    }
    parse_status(&resp.body, link.host())
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! fixture {
        ($name:literal) => {
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/data/services/",
                $name
            ))
        };
    }

    #[test]
    fn recorded_answers_map_to_verdicts() {
        let cases: [(&[u8], &str, Verdict); 5] = [
            (fixture!("sb_example.com.txt"), "example.com", Verdict::Safe),
            (
                fixture!("sb_testsafebrowsing.appspot.com.txt"),
                "testsafebrowsing.appspot.com",
                Verdict::Unsafe,
            ),
            (
                fixture!("sb_malware.testing.google.test.txt"),
                "malware.testing.google.test",
                Verdict::Unsafe,
            ),
            (
                fixture!("sb_github.com.txt"),
                "github.com",
                Verdict::Unknown,
            ),
            (
                fixture!("sb_nonexistent-host-zzzz.invalid.txt"),
                "nonexistent-host-zzzz.invalid",
                Verdict::Unknown,
            ),
        ];
        for (body, host, want) in cases {
            assert_eq!(parse_status(body, host), Ok(want), "{host}");
        }
    }

    #[test]
    fn odd_answers_are_format_errors() {
        for bad in [
            &b""[..],
            b")]}'\n",
            b")]}'\n<html>",
            b")]}'\n[]",
            b")]}'\n[[\"other\",1,false,false,false,false,false,0,\"a.com\",false]]",
            b")]}'\n[[\"sb.ssr\",\"1\",false,false,false,false,false,0,\"a.com\",false]]",
            b")]}'\n[[\"sb.ssr\",1,false,false]]",
            b")]}'\n[[\"sb.ssr\",1,false,false,false,false,false,0,\"b.com\",false]]",
            b"\xff\xfe",
        ] {
            assert_eq!(
                parse_status(bad, "a.com"),
                Err(CheckError::Format),
                "{bad:?}"
            );
        }
        // Without the prefix, and with a status not seen yet, still read.
        assert_eq!(
            parse_status(
                b"[[\"sb.ssr\",2,false,false,false,false,false,0,\"a.com\",false]]",
                "a.com"
            ),
            Ok(Verdict::Unsafe)
        );
        assert_eq!(
            parse_status(
                b")]}'\n[[\"sb.ssr\",5,false,false,false,false,false,0,\"a.com\",false]]",
                "a.com"
            ),
            Ok(Verdict::Unknown)
        );
    }

    #[test]
    fn hosts_are_what_a_browser_contacts() {
        let host = |s: &str| WebLink::parse(s).map(|l| l.host().to_owned());
        assert_eq!(
            host("https://Example.COM/a?b#c").as_deref(),
            Some("example.com")
        );
        assert_eq!(host("http://example.com").as_deref(), Some("example.com"));
        assert_eq!(
            host("HTTPS://example.com.:8443/x").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            host("https://good.com@evil.com/").as_deref(),
            Some("evil.com")
        );
        assert_eq!(
            host("https://a:b@good.com:80@evil.com").as_deref(),
            Some("evil.com")
        );
        assert_eq!(
            host("https://example.com?x=@y.com").as_deref(),
            Some("example.com")
        );
        assert_eq!(host("https://[::1]:8080/").as_deref(), Some("[::1]"));
        assert_eq!(
            host("  https://example.com/  ").as_deref(),
            Some("example.com")
        );
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "ftp://example.com/",
            "mailto:a@b.c",
            "https://",
            "https:///path",
            "https://exa mple.com/",
            "https://example.com\\@evil.com/",
            "https://evil.com\\.example.com/",
            "https://example.com:port/",
            "https://example.com:123456/",
            "https://example.com/\u{0}",
            "https://example.com/a\nb",
            "//example.com",
            "https://..com/",
        ] {
            assert_eq!(WebLink::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn local_and_numeric_hosts_are_never_sent() {
        let checkable = |s: &str| WebLink::parse(s).unwrap().checkable();
        assert!(checkable("https://example.com/"));
        assert!(checkable("https://sub.example.co.uk/"));
        for local in [
            "http://localhost:8080/",
            "http://router/",
            "http://192.168.1.1/",
            "http://[fe80::1]/",
            "http://printer.local/",
            "http://app.localhost/",
            "https://bücher.example/",
        ] {
            assert!(!checkable(local), "{local}");
        }
    }

    #[test]
    fn check_asks_google_about_the_host_only() {
        use std::sync::Arc;
        let fake = Arc::new(crate::gifs::tests::Fake::answering(vec![(
            "https://transparencyreport.google.com/",
            200,
            fixture!("sb_testsafebrowsing.appspot.com.txt").to_vec(),
        )]));
        let http = HttpClient::new(
            Arc::clone(&fake) as Arc<dyn crate::artwork::http::Transport>,
            Arc::new(crate::clock::SystemClock::new()),
        )
        .for_hosts(HOSTS);
        let link =
            WebLink::parse("https://testsafebrowsing.appspot.com/s/malware.html?x=1").unwrap();
        assert_eq!(check(&http, &link), Ok(Verdict::Unsafe));
        let asked = fake.asked.lock().unwrap().clone();
        // Directly, never through the GIF relay.
        assert_eq!(
            asked,
            [format!(
                "direct {}",
                status_url("testsafebrowsing.appspot.com")
            )]
        );
        // No answer is an error, never "safe".
        let other = WebLink::parse("https://example.com/").unwrap();
        let offline = HttpClient::new(
            Arc::new(crate::gifs::tests::Fake::default()),
            Arc::new(crate::clock::SystemClock::new()),
        )
        .for_hosts(HOSTS);
        assert!(matches!(
            check(&offline, &other),
            Err(CheckError::Network(_))
        ));
    }

    #[test]
    fn steps() {
        let step_of = |check: bool, s: &str| step(check, &WebLink::parse(s).unwrap());
        assert_eq!(step_of(false, "https://evil.example/"), LinkStep::Open);
        assert_eq!(step_of(true, "https://example.com/a"), LinkStep::Check);
        assert_eq!(step_of(true, "https://xn--bcher-kva.ch/"), LinkStep::Check);
        assert_eq!(step_of(true, "http://192.168.1.1/"), LinkStep::Open);
        assert_eq!(step_of(true, "http://router/"), LinkStep::Open);
        assert_eq!(step_of(true, "http://[::1]:8080/"), LinkStep::Open);
        assert_eq!(step_of(true, "http://printer.local/"), LinkStep::Open);
        // A look-alike name in other characters is never opened unchecked.
        assert_eq!(step_of(true, "https://аpple.com/"), LinkStep::Ask);
        assert_eq!(step_of(true, "https://bücher.example/"), LinkStep::Ask);
        assert_eq!(step_of(true, "https://exa%6dple.com/"), LinkStep::Ask);
    }

    #[test]
    fn only_the_host_is_in_the_request() {
        assert_eq!(
            status_url("example.com"),
            "https://transparencyreport.google.com/transparencyreport/api/v3/safebrowsing/status?site=example.com"
        );
        let link = WebLink::parse("https://example.com/private/path?token=secret").unwrap();
        let url = status_url(link.host());
        assert!(!url.contains("private") && !url.contains("secret"));
    }
}
