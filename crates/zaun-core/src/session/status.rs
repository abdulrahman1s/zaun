//! Parsers of the two status streams the supervisor reads (PROC-002): bwrap's
//! `--json-status-fd` objects [V: F-211] and the init shim's line protocol
//! (PROCESS_LIFECYCLE §4). Both are incremental (any split of the input gives the same
//! result), bounded in memory, and ignore what they do not understand.

use serde_json::Value;

/// Largest buffered fragment; a longer object or line is discarded.
pub const MAX_STATUS_FRAGMENT: usize = 64 * 1024;

/// One event from bwrap's status descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BwrapStatus {
    /// The sandbox's PID 1 (host PID) and its PID namespace.
    ChildPid {
        /// Host PID of the sandbox's PID 1.
        pid: u32,
        /// `pid-namespace` inode, when reported.
        pid_ns: Option<u64>,
    },
    /// The exit status of the sandboxed command.
    ExitCode(i32),
}

/// Incremental parser of `--json-status-fd` output: a sequence of JSON objects, one per line
/// in practice, tolerated across lines and concatenated. Unknown members are ignored.
#[derive(Debug, Default)]
pub struct BwrapStatusParser {
    buf: Vec<u8>,
}

impl BwrapStatusParser {
    /// A new parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes; returns the complete events.
    pub fn push(&mut self, data: &[u8]) -> Vec<BwrapStatus> {
        self.buf.extend_from_slice(data);
        let mut out = Vec::new();
        loop {
            let start = match self.buf.iter().position(|b| *b == b'{') {
                Some(i) => i,
                None => {
                    self.buf.clear();
                    break;
                }
            };
            let mut it =
                serde_json::Deserializer::from_slice(&self.buf[start..]).into_iter::<Value>();
            match it.next() {
                Some(Ok(v)) => {
                    let used = start + it.byte_offset();
                    out.extend(Self::events(&v));
                    self.buf.drain(..used);
                }
                Some(Err(e)) if e.is_eof() => {
                    if self.buf.len() - start > MAX_STATUS_FRAGMENT {
                        self.buf.clear();
                    } else {
                        self.buf.drain(..start);
                    }
                    break;
                }
                Some(Err(_)) => {
                    // Garbage: skip past this brace and resynchronise.
                    self.buf.drain(..=start);
                }
                None => {
                    self.buf.clear();
                    break;
                }
            }
        }
        out
    }

    fn events(v: &Value) -> Vec<BwrapStatus> {
        let mut out = Vec::new();
        let Some(obj) = v.as_object() else {
            return out;
        };
        if let Some(pid) = obj
            .get("child-pid")
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok())
        {
            out.push(BwrapStatus::ChildPid {
                pid,
                pid_ns: obj.get("pid-namespace").and_then(Value::as_u64),
            });
        }
        if let Some(code) = obj
            .get("exit-code")
            .and_then(Value::as_i64)
            .and_then(|c| i32::try_from(c).ok())
        {
            out.push(BwrapStatus::ExitCode(code));
        }
        out
    }
}

/// One line of the init shim's status protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitStatus {
    /// `started <pid>` (PID inside the sandbox).
    Started(u32),
    /// `main-exit <status>` (exit code, or 128 + signal).
    MainExit(i32),
    /// `stop-requested <signo>`.
    StopRequested(i32),
    /// `exec-failed <errno>`.
    ExecFailed(i32),
    /// `empty`: no process is left.
    Empty,
}

impl InitStatus {
    /// Parse one line (without the newline); `None` for anything unknown.
    pub fn parse(line: &str) -> Option<Self> {
        let mut it = line.trim().split(' ');
        let word = it.next()?;
        let num = it.next();
        if it.next().is_some() {
            return None;
        }
        let n = || num.and_then(|n| n.parse::<i64>().ok());
        Some(match (word, num) {
            ("empty", None) => InitStatus::Empty,
            ("started", Some(_)) => InitStatus::Started(u32::try_from(n()?).ok()?),
            ("main-exit", Some(_)) => InitStatus::MainExit(i32::try_from(n()?).ok()?),
            ("stop-requested", Some(_)) => InitStatus::StopRequested(i32::try_from(n()?).ok()?),
            ("exec-failed", Some(_)) => InitStatus::ExecFailed(i32::try_from(n()?).ok()?),
            _ => return None,
        })
    }
}

/// Incremental parser of the init shim's status lines.
#[derive(Debug, Default)]
pub struct InitStatusParser {
    buf: Vec<u8>,
    discarding: bool,
}

impl InitStatusParser {
    /// A new parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes; returns the recognised lines.
    pub fn push(&mut self, data: &[u8]) -> Vec<InitStatus> {
        let mut out = Vec::new();
        for &b in data {
            if b == b'\n' {
                if !self.discarding
                    && let Ok(s) = std::str::from_utf8(&self.buf)
                    && let Some(st) = InitStatus::parse(s)
                {
                    out.push(st);
                }
                self.buf.clear();
                self.discarding = false;
            } else if !self.discarding {
                if self.buf.len() >= 256 {
                    self.buf.clear();
                    self.discarding = true;
                } else {
                    self.buf.push(b);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &[u8] = b"{ \"child-pid\": 4242, \"cgroup-namespace\": 4026531835, \"ipc-namespace\": 4026531839, \"mnt-namespace\": 4026533001, \"net-namespace\": 4026533004, \"pid-namespace\": 4026533002, \"uts-namespace\": 4026533003 }\n{ \"exit-code\": 7 }\n";

    #[test]
    fn real_output_parses_in_any_split() {
        let want = vec![
            BwrapStatus::ChildPid {
                pid: 4242,
                pid_ns: Some(4_026_533_002),
            },
            BwrapStatus::ExitCode(7),
        ];
        for cut in 0..REAL.len() {
            let mut p = BwrapStatusParser::new();
            let mut got = p.push(&REAL[..cut]);
            got.extend(p.push(&REAL[cut..]));
            assert_eq!(got, want, "cut at {cut}");
        }
        let mut p = BwrapStatusParser::new();
        let mut got = Vec::new();
        for b in REAL {
            got.extend(p.push(std::slice::from_ref(b)));
        }
        assert_eq!(got, want);
    }

    #[test]
    fn bwrap_garbage_is_ignored() {
        let mut p = BwrapStatusParser::new();
        assert!(p.push(b"not json\n{]\n{\"x\": 1}\n").is_empty());
        assert_eq!(
            p.push(b"junk { \"exit-code\": 0 }"),
            vec![BwrapStatus::ExitCode(0)]
        );
        assert!(
            p.push(b"{\"child-pid\": -1}{\"child-pid\": \"a\"}")
                .is_empty()
        );
        // Unterminated huge fragment is dropped.
        let mut big = b"{\"a\": \"".to_vec();
        big.extend(std::iter::repeat_n(b'x', MAX_STATUS_FRAGMENT + 10));
        assert!(p.push(&big).is_empty());
        assert_eq!(
            p.push(b"{\"exit-code\": 3}"),
            vec![BwrapStatus::ExitCode(3)]
        );
    }

    #[test]
    fn init_lines() {
        let mut p = InitStatusParser::new();
        let text = b"started 2\nmain-exit 143\nstop-requested 15\nexec-failed 2\nempty\n";
        let mut got = Vec::new();
        for chunk in text.chunks(3) {
            got.extend(p.push(chunk));
        }
        assert_eq!(
            got,
            vec![
                InitStatus::Started(2),
                InitStatus::MainExit(143),
                InitStatus::StopRequested(15),
                InitStatus::ExecFailed(2),
                InitStatus::Empty
            ]
        );
        assert!(
            p.push(b"started\nempty 1\nfoo 2\nmain-exit x\nstarted -1\n")
                .is_empty()
        );
        let mut long = vec![b'a'; 1000];
        long.extend(b"\nempty\n");
        assert_eq!(p.push(&long), vec![InitStatus::Empty]);
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..200 {
            let mut bytes = Vec::new();
            for _ in 0..512 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                bytes.push(b"{}\"0123456789:, \nabcdefghij-exitcode"[(seed % 36) as usize]);
            }
            let _ = BwrapStatusParser::new().push(&bytes);
            let _ = InitStatusParser::new().push(&bytes);
        }
    }
}
