//! Semantic versions and their order (RELEASE_STRATEGY §1), enough to refuse downgrades.

use std::cmp::Ordering;
use std::fmt;

/// `MAJOR.MINOR.PATCH[-PRE][+BUILD]`; build metadata is ignored when comparing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Dot-separated pre-release identifiers (`beta.2` → `["beta", "2"]`).
    pub pre: Vec<String>,
}

impl Version {
    /// Parse a version; `None` for anything else (a leading `v` is accepted).
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().strip_prefix('v').unwrap_or(s.trim());
        let core_pre = s.split_once('+').map_or(s, |(a, _)| a);
        let (core, pre) = match core_pre.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (core_pre, None),
        };
        let mut nums = core.split('.').map(number);
        let (major, minor, patch) = (nums.next()??, nums.next()??, nums.next()??);
        if nums.next().is_some() {
            return None;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(p) => {
                let ids: Vec<String> = p.split('.').map(str::to_owned).collect();
                let ok = |i: &String| {
                    !i.is_empty() && i.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                };
                if !ids.iter().all(ok) {
                    return None;
                }
                ids
            }
        };
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }
}

fn number(s: &str) -> Option<u64> {
    let ok =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    ok.then(|| s.parse().ok()).flatten()
}

fn cmp_ident(a: &str, b: &str) -> Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                // A pre-release comes before its release.
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (a, b) in self.pre.iter().zip(&other.pre) {
                        let o = cmp_ident(a, b);
                        if o != Ordering::Equal {
                            return o;
                        }
                    }
                    self.pre.len().cmp(&other.pre.len())
                }
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn precedence_follows_semver() {
        let order = [
            "0.9.9",
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
            "1.0.1",
            "1.10.0",
        ];
        for w in order.windows(2) {
            assert!(v(w[0]) < v(w[1]), "{} < {}", w[0], w[1]);
        }
        assert_eq!(v("v1.2.3+build.7"), v("1.2.3"));
    }

    #[test]
    fn malformed_versions_are_refused() {
        for bad in [
            "",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.x",
            "1.2.3-",
            "1.2.3-a..b",
            "1.2.3-ä",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert_eq!(v("1.2.3-rc.1").to_string(), "1.2.3-rc.1");
    }
}
