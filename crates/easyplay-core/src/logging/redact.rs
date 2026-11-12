//! One redaction function used everywhere (LOGGING §2.5, PRIVACY §5).
//!
//! Applied to every `path` log field, to `UserError.details`, and to diagnostics reports. It
//! replaces the home directory prefix with `~`, the user name as a path component with
//! `<user>`, and the host name as a word with `<host>`. Redaction is for sharing convenience,
//! not a security boundary. It is idempotent.

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

/// Replaces personal identifiers in text and paths.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Redactor {
    home: Option<Vec<u8>>,
    user: Option<Vec<u8>>,
    host: Option<Vec<u8>>,
}

const HOME_MARK: &[u8] = b"~";
const USER_MARK: &[u8] = b"<user>";
const HOST_MARK: &[u8] = b"<host>";

impl Redactor {
    /// A redactor for this home directory, user name and host name. Empty values, a home of
    /// `/`, and names shorter than two bytes are ignored (they would match too much).
    pub fn new(home: Option<&Path>, user: Option<&str>, host: Option<&str>) -> Self {
        let home = home
            .map(|h| {
                let mut b = h.as_os_str().as_bytes().to_vec();
                while b.len() > 1 && b.ends_with(b"/") {
                    b.pop();
                }
                b
            })
            .filter(|b| b.len() > 1 && b.starts_with(b"/"));
        let name = |s: Option<&str>| {
            s.map(|s| s.as_bytes().to_vec())
                .filter(|b| b.len() >= 2 && !b.contains(&b'/'))
        };
        Self {
            home,
            user: name(user),
            host: name(host),
        }
    }

    /// A redactor that changes nothing.
    pub fn none() -> Self {
        Self::default()
    }

    /// Redact text.
    pub fn redact_str(&self, s: &str) -> String {
        let out = self.redact_bytes(s.as_bytes());
        // Replacements are ASCII and only replace whole UTF-8 sequences, so the result is
        // valid UTF-8; fall back to lossy conversion defensively.
        String::from_utf8(out)
            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
    }

    /// Redact a path, which may not be UTF-8; the result is displayable text.
    pub fn redact_path(&self, p: &Path) -> String {
        String::from_utf8_lossy(&self.redact_bytes(p.as_os_str().as_bytes())).into_owned()
    }

    /// Redact raw bytes.
    pub fn redact_bytes(&self, input: &[u8]) -> Vec<u8> {
        let mut out = input.to_vec();
        if let Some(home) = &self.home {
            out = replace_home(&out, home);
        }
        if let Some(user) = &self.user {
            out = replace_where(&out, user, USER_MARK, |before, after| {
                before == Some(b'/') && matches!(after, None | Some(b'/'))
            });
        }
        if let Some(host) = &self.host {
            out = replace_where(&out, host, HOST_MARK, |before, after| {
                let word = |c: Option<u8>| {
                    c.is_some_and(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                };
                !word(before) && !word(after) && before != Some(b'<')
            });
        }
        out
    }
}

/// Replace `home` where it starts a path: at the beginning of the text or after a character
/// that cannot be part of a path component, and followed by `/` or the end of a path.
fn replace_home(input: &[u8], home: &[u8]) -> Vec<u8> {
    replace_where(input, home, HOME_MARK, |before, after| {
        let starts = before.is_none_or(|c| {
            c.is_ascii_whitespace()
                || matches!(
                    c,
                    b'"' | b'\'' | b'=' | b':' | b'(' | b'[' | b',' | b'<' | b'`'
                )
        });
        let ends = after.is_none_or(|c| {
            c == b'/'
                || c.is_ascii_whitespace()
                || matches!(
                    c,
                    b'"' | b'\'' | b':' | b')' | b']' | b',' | b'>' | b'`' | b'.'
                )
        });
        starts && ends
    })
}

fn replace_where(
    input: &[u8],
    needle: &[u8],
    mark: &[u8],
    accept: impl Fn(Option<u8>, Option<u8>) -> bool,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i..].starts_with(needle) {
            let before = i.checked_sub(1).map(|j| input[j]);
            let after = input.get(i + needle.len()).copied();
            if accept(before, after) {
                out.extend_from_slice(mark);
                i += needle.len();
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn r() -> Redactor {
        Redactor::new(Some(Path::new("/home/sam/")), Some("sam"), Some("gamebox"))
    }

    #[test]
    fn redaction_table() {
        let cases = [
            ("/home/sam", "~"),
            ("/home/sam/Games/x.exe", "~/Games/x.exe"),
            ("path=/home/sam/a", "path=~/a"),
            ("\"/home/sam/a b\"", "\"~/a b\""),
            ("/home/samuel/x", "/home/samuel/x"),
            // Not the home prefix, but the user name is still a path component.
            ("/data/home/sam/x", "/data/home/<user>/x"),
            ("/run/media/sam/USB", "/run/media/<user>/USB"),
            ("/var/lib/sam", "/var/lib/<user>"),
            ("sam played", "sam played"),
            ("/home/sam/sam/x", "~/<user>/x"),
            ("connected to gamebox:4713", "connected to <host>:4713"),
            ("gameboxes", "gameboxes"),
            ("my-gamebox", "my-gamebox"),
            ("cannot read /home/sam: denied", "cannot read ~: denied"),
            ("", ""),
        ];
        for (input, expected) in cases {
            assert_eq!(r().redact_str(input), expected, "{input:?}");
        }
    }

    #[test]
    fn idempotent() {
        let inputs = [
            "/home/sam/Games and /run/media/sam/x on gamebox",
            "~/x <user> <host>",
        ];
        for input in inputs {
            let once = r().redact_str(input);
            assert_eq!(r().redact_str(&once), once);
        }
        // A user called "user" and a host called "host" do not re-match their own marks.
        let odd = Redactor::new(Some(Path::new("/home/user")), Some("user"), Some("host"));
        let once = odd.redact_str("/run/media/user/x host");
        assert_eq!(once, "/run/media/<user>/x <host>");
        assert_eq!(odd.redact_str(&once), once);
    }

    #[test]
    fn non_utf8_paths() {
        let p = Path::new(OsStr::from_bytes(b"/home/sam/Spiele/\xff\xfe.exe"));
        let out = r().redact_path(p);
        assert!(out.starts_with("~/Spiele/"), "{out}");
        assert!(out.contains('\u{fffd}'));
        assert_eq!(r().redact_bytes(b"/home/sam/\xff"), b"~/\xff");
    }

    #[test]
    fn ignores_useless_inputs() {
        let n = Redactor::new(Some(Path::new("/")), Some(""), Some("x"));
        assert_eq!(n.redact_str("/x/y x"), "/x/y x");
        assert_eq!(Redactor::none().redact_str("/home/sam"), "/home/sam");
    }
}
