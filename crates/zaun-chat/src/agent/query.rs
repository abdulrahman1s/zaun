//! Query strings of connection links: http-types `renderSimpleQuery False` and `parseSimpleQuery` (Agent/QueryString.hs, simplexmq 7.1.4, AGPL-3.0-only).
use std::fmt::Write;

pub type Query = Vec<(String, String)>;

#[must_use]
pub fn url_encode(s: &str) -> String {
    s.bytes()
        .fold(String::with_capacity(s.len()), |mut out, b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                out.push(char::from(b));
            } else {
                let _ = write!(out, "%{b:02X}"); // writing to a String cannot fail
            }
            out
        })
}

fn hex(b: u8) -> Option<u8> {
    char::from(b)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

/// `urlDecode True`: `%XX` and `+` as a space; bad escapes stay as they are.
#[must_use]
pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let escaped = (c == b'%')
            .then(|| Some(hex(*b.get(i + 1)?)? << 4 | hex(*b.get(i + 2)?)?))
            .flatten();
        match (escaped, c) {
            (Some(v), _) => {
                out.push(v);
                i += 3;
            }
            (None, b'+') => {
                out.push(b' ');
                i += 1;
            }
            (None, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `QEscape`: names and values are percent-encoded.
#[must_use]
pub fn render(q: &[(String, String)]) -> String {
    join(q, url_encode)
}

/// `QNoEscaping`: values are written as they are.
#[must_use]
pub fn render_raw(q: &[(String, String)]) -> String {
    join(q, str::to_owned)
}

fn join(q: &[(String, String)], value: impl Fn(&str) -> String) -> String {
    let pairs: Vec<String> = q
        .iter()
        .map(|(k, v)| format!("{}={}", url_encode(k), value(v)))
        .collect();
    pairs.join("&")
}

#[must_use]
pub fn parse(s: &str) -> Query {
    s.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (url_decode(k), url_decode(v)),
            None => (url_decode(p), String::new()),
        })
        .collect()
}

#[must_use]
pub fn get<'a>(q: &'a [(String, String)], name: &str) -> Option<&'a str> {
    q.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

pub fn pair(name: &str, value: impl Into<String>) -> (String, String) {
    (name.to_owned(), value.into())
}
