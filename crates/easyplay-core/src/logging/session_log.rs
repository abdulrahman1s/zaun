//! Per-session `game.log` writing with framing, sanitising, noise dropping, repetition
//! collapse and a byte cap (LOGGING §3.1). Session output is **untrusted**: it is never
//! interpolated into the application log.

use std::io::{self, Write};
use std::time::Duration;

use crate::consts::{LOG_LINE_MAX_BYTES, SESSION_LOG_CAP_DEFAULT, SESSION_LOG_REPEAT_LIMIT};

/// Marker appended to a line cut at [`LOG_LINE_MAX_BYTES`].
pub const TRUNCATED_MARK: &str = "…[truncated]";

/// A line produced by [`LineSplitter`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FramedLine {
    /// Raw bytes without the newline (at most [`LOG_LINE_MAX_BYTES`]).
    pub bytes: Vec<u8>,
    /// The original line was longer and was cut.
    pub truncated: bool,
}

/// Splits a byte stream into lines of bounded length.
#[derive(Debug, Default)]
pub struct LineSplitter {
    buf: Vec<u8>,
    discarding: bool,
}

impl LineSplitter {
    /// A new splitter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed bytes; returns the complete lines.
    pub fn push(&mut self, mut data: &[u8]) -> Vec<FramedLine> {
        let mut out = Vec::new();
        while !data.is_empty() {
            match data.iter().position(|b| *b == b'\n') {
                Some(nl) => {
                    let (head, rest) = data.split_at(nl);
                    if !self.discarding {
                        self.append(head, &mut out);
                    }
                    if self.discarding {
                        self.discarding = false;
                    } else {
                        out.push(FramedLine {
                            bytes: std::mem::take(&mut self.buf),
                            truncated: false,
                        });
                    }
                    data = &rest[1..];
                }
                None => {
                    if !self.discarding {
                        self.append(data, &mut out);
                    }
                    data = &[];
                }
            }
        }
        out
    }

    fn append(&mut self, chunk: &[u8], out: &mut Vec<FramedLine>) {
        let room = LOG_LINE_MAX_BYTES - self.buf.len();
        if chunk.len() > room {
            self.buf.extend_from_slice(&chunk[..room]);
            out.push(FramedLine {
                bytes: std::mem::take(&mut self.buf),
                truncated: true,
            });
            self.discarding = true;
        } else {
            self.buf.extend_from_slice(chunk);
        }
    }

    /// The partial last line, if any (call at end of stream).
    pub fn finish(&mut self) -> Option<FramedLine> {
        if self.buf.is_empty() || self.discarding {
            self.discarding = false;
            self.buf.clear();
            return None;
        }
        Some(FramedLine {
            bytes: std::mem::take(&mut self.buf),
            truncated: false,
        })
    }
}

/// Remove ANSI escape sequences, replace control characters other than tab with `·`, and
/// replace invalid UTF-8 with U+FFFD.
pub fn sanitize_line(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                // CSI: ESC [ parameters… final byte 0x40–0x7E.
                Some('[') => {
                    chars.next();
                    for d in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&d) {
                            break;
                        }
                    }
                }
                // OSC: ESC ] … BEL or ESC \.
                Some(']') => {
                    chars.next();
                    while let Some(d) = chars.next() {
                        if d == '\u{7}' {
                            break;
                        }
                        if d == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                // Two-character sequence.
                Some(_) => {
                    chars.next();
                }
                None => {}
            }
            continue;
        }
        if c == '\t' || !c.is_control() {
            out.push(c);
        } else {
            out.push('·');
        }
    }
    out
}

/// A class of noise lines that are counted instead of written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropPattern {
    /// Description used in the summary line.
    pub label: String,
    /// A line containing this text matches.
    pub needle: String,
}

/// Configuration of a [`SessionLogWriter`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionLogConfig {
    /// Byte cap of the file (`logging.session_log_cap_mb`).
    pub cap_bytes: u64,
    /// Noise patterns.
    pub drop_patterns: Vec<DropPattern>,
    /// Identical consecutive lines written before collapsing.
    pub repeat_limit: u32,
}

impl Default for SessionLogConfig {
    fn default() -> Self {
        Self {
            cap_bytes: SESSION_LOG_CAP_DEFAULT,
            drop_patterns: Vec::new(),
            repeat_limit: SESSION_LOG_REPEAT_LIMIT,
        }
    }
}

/// What happened to a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineOutcome {
    /// Written.
    Written,
    /// Matched a noise pattern; counted.
    Dropped,
    /// An identical consecutive line beyond the repetition limit; counted.
    Collapsed,
    /// The cap was reached; discarded.
    OverCap,
}

/// Writes sanitised lines with elapsed-time prefixes to `W`, enforcing the rules of
/// LOGGING §3.1.
#[derive(Debug)]
pub struct SessionLogWriter<W: Write> {
    out: W,
    cfg: SessionLogConfig,
    written: u64,
    capped: bool,
    last: Option<String>,
    run: u32,
    suppressed: u64,
    drop_counts: Vec<u64>,
}

/// Format `n` with thousands separators (`3,412`).
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn format_cap(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{bytes} bytes")
    }
}

impl<W: Write> SessionLogWriter<W> {
    /// A writer appending to `out`.
    pub fn new(out: W, cfg: SessionLogConfig) -> Self {
        let n = cfg.drop_patterns.len();
        Self {
            out,
            cfg,
            written: 0,
            capped: false,
            last: None,
            run: 0,
            suppressed: 0,
            drop_counts: vec![0; n],
        }
    }

    /// Bytes written so far.
    pub fn bytes_written(&self) -> u64 {
        self.written
    }

    /// Whether the cap was reached.
    pub fn is_capped(&self) -> bool {
        self.capped
    }

    fn raw(&mut self, text: &str) -> io::Result<()> {
        self.out.write_all(text.as_bytes())?;
        self.out.write_all(b"\n")?;
        self.written += text.len() as u64 + 1;
        Ok(())
    }

    fn flush_repeats(&mut self) -> io::Result<()> {
        if self.suppressed > 0 {
            let n = self.suppressed;
            self.suppressed = 0;
            self.raw(&format!("[previous line repeated {} times]", thousands(n)))?;
        }
        Ok(())
    }

    /// Write one framed line received `elapsed` after the session started.
    pub fn write_line(&mut self, elapsed: Duration, line: &FramedLine) -> io::Result<LineOutcome> {
        let mut text = sanitize_line(&line.bytes);
        if line.truncated {
            text.push_str(TRUNCATED_MARK);
        }
        if let Some(i) = self
            .cfg
            .drop_patterns
            .iter()
            .position(|p| text.contains(p.needle.as_str()))
        {
            self.drop_counts[i] += 1;
            return Ok(LineOutcome::Dropped);
        }
        if self.capped {
            return Ok(LineOutcome::OverCap);
        }
        if self.last.as_deref() == Some(text.as_str()) {
            self.run = self.run.saturating_add(1);
            if self.run > self.cfg.repeat_limit {
                self.suppressed += 1;
                return Ok(LineOutcome::Collapsed);
            }
        } else {
            self.flush_repeats()?;
            self.run = 1;
            self.last = Some(text.clone());
        }
        let stamped = format!(
            "[+{}.{:03}] {text}",
            elapsed.as_secs(),
            elapsed.subsec_millis()
        );
        if self.written + stamped.len() as u64 + 1 > self.cfg.cap_bytes {
            self.capped = true;
            self.raw(&format!(
                "[log truncated at {}]",
                format_cap(self.cfg.cap_bytes)
            ))?;
            return Ok(LineOutcome::OverCap);
        }
        self.raw(&stamped)?;
        Ok(LineOutcome::Written)
    }

    /// Write the closing notes (pending repetition, hidden-line counts) and return the sink.
    pub fn finish(mut self) -> io::Result<W> {
        if !self.capped {
            self.flush_repeats()?;
        }
        for i in 0..self.drop_counts.len() {
            let n = self.drop_counts[i];
            if n > 0 {
                let label = self.cfg.drop_patterns[i].label.clone();
                self.raw(&format!("[{n} similar lines hidden: {label}]"))?;
            }
        }
        self.out.flush()?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(s: &str) -> FramedLine {
        FramedLine {
            bytes: s.as_bytes().to_vec(),
            truncated: false,
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn framing() {
        let mut s = LineSplitter::new();
        assert_eq!(s.push(b"par"), vec![]);
        let lines = s.push(b"tial\nnext\n\nlast");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].bytes, b"partial");
        assert_eq!(lines[1].bytes, b"next");
        assert_eq!(lines[2].bytes, b"");
        assert_eq!(s.finish().unwrap().bytes, b"last");
        assert_eq!(s.finish(), None);
    }

    #[test]
    fn long_lines_are_cut_once() {
        let mut s = LineSplitter::new();
        let mut data = vec![b'x'; 5000];
        data.extend_from_slice(b"\nok\n");
        let lines = s.push(&data[..3000]);
        assert!(lines.is_empty());
        let lines = s.push(&data[3000..]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].bytes.len(), LOG_LINE_MAX_BYTES);
        assert!(lines[0].truncated);
        assert_eq!(lines[1].bytes, b"ok");
    }

    #[test]
    fn sanitising() {
        assert_eq!(sanitize_line(b"\x1b[31mred\x1b[0m"), "red");
        assert_eq!(sanitize_line(b"\x1b]0;title\x07after"), "after");
        assert_eq!(sanitize_line(b"\x1b]8;;http://x\x1b\\link"), "link");
        assert_eq!(
            sanitize_line(b"nul\0bell\x07tab\tcr\r"),
            "nul·bell·tab\tcr·"
        );
        assert_eq!(sanitize_line(b"bad \xff utf8"), "bad \u{fffd} utf8");
        assert_eq!(sanitize_line(b"\x1bc reset"), " reset");
    }

    #[test]
    fn prefix_and_repetition() {
        let mut w = SessionLogWriter::new(
            Vec::new(),
            SessionLogConfig {
                repeat_limit: 2,
                ..SessionLogConfig::default()
            },
        );
        assert_eq!(
            w.write_line(ms(12_345), &line("start")).unwrap(),
            LineOutcome::Written
        );
        for _ in 0..5 {
            w.write_line(ms(13_000), &line("spam")).unwrap();
        }
        w.write_line(ms(14_000), &line("end")).unwrap();
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        assert_eq!(
            out,
            "[+12.345] start\n[+13.000] spam\n[+13.000] spam\n[previous line repeated 3 times]\n[+14.000] end\n"
        );
    }

    #[test]
    fn drop_patterns_are_counted() {
        let mut w = SessionLogWriter::new(
            Vec::new(),
            SessionLogConfig {
                drop_patterns: vec![DropPattern {
                    label: "pressure-vessel hard-link fallback".into(),
                    needle: "Unable to create hard link".into(),
                }],
                ..SessionLogConfig::default()
            },
        );
        for _ in 0..3 {
            assert_eq!(
                w.write_line(ms(1), &line("pv: Unable to create hard link x"))
                    .unwrap(),
                LineOutcome::Dropped
            );
        }
        w.write_line(ms(2), &line("real")).unwrap();
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        assert_eq!(
            out,
            "[+0.002] real\n[3 similar lines hidden: pressure-vessel hard-link fallback]\n"
        );
    }

    #[test]
    fn cap_is_enforced_and_noted_once() {
        let mut w = SessionLogWriter::new(
            Vec::new(),
            SessionLogConfig {
                cap_bytes: 40,
                ..SessionLogConfig::default()
            },
        );
        assert_eq!(
            w.write_line(ms(0), &line("first line")).unwrap(),
            LineOutcome::Written
        );
        assert_eq!(
            w.write_line(ms(0), &line("second line is long")).unwrap(),
            LineOutcome::OverCap
        );
        assert!(w.is_capped());
        assert_eq!(
            w.write_line(ms(0), &line("x")).unwrap(),
            LineOutcome::OverCap
        );
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        assert_eq!(out, "[+0.000] first line\n[log truncated at 40 bytes]\n");
    }

    #[test]
    fn hostile_lines_are_plain_text() {
        let mut w = SessionLogWriter::new(Vec::new(), SessionLogConfig::default());
        let forged = FramedLine {
            bytes: b"2026-11-12T21:04:11.203Z INFO  gui[1] fake\x1b[2K\r".to_vec(),
            truncated: true,
        };
        w.write_line(ms(5), &forged).unwrap();
        let out = String::from_utf8(w.finish().unwrap()).unwrap();
        assert!(
            out.starts_with("[+0.005] 2026-11-12T21:04:11.203Z INFO  gui[1] fake·…[truncated]")
        );
        assert_eq!(out.lines().count(), 1);
    }

    #[test]
    fn thousands_separator() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(3412), "3,412");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
