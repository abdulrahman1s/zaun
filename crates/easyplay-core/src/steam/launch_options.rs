//! A Steam game's launch options (ADR-0031 §6a): the per-app `LaunchOptions` value in the
//! account's `userdata/<account id>/config/localconfig.vdf`, at
//! `UserLocalConfigStore > Software > Valve > Steam > apps > <appid> > LaunchOptions` — the
//! same setting as Steam › Properties › Launch options.
//!
//! This is the one Steam file EasyPlay changes, and only this one value in it:
//!
//! - **Only while Steam is closed** ([`super::client::client_running`]): Steam rewrites the
//!   file from memory when it exits, so a change made while it runs would be lost.
//! - **The rest of the file stays byte for byte as it was.** [`set_value`] is a minimal
//!   editor over the reader's own tokenizer ([`super::keyvalues`]): it replaces the bytes
//!   of one value, or inserts the missing `"<appid>" { "LaunchOptions" "…" }` block (and any
//!   missing parent) before the closing brace of the deepest block that exists, with the
//!   file's own indentation and line ending. A file the tolerant reader had to repair is
//!   not edited at all.
//! - **A backup and an atomic write**: the file as it was goes to
//!   `localconfig.vdf.easyplay-bak` next to it (one, replaced each time), then the new text
//!   replaces the file through a temporary file and a rename, with the same permissions.
//!
//! The account is the one whose play time EasyPlay shows ([`super::account::localconfig_path`]:
//! the most recently used one) in the installation that lists the game.
//!
//! [`with_mangohud`] and [`has_mangohud`] turn the performance overlay on and off in such a
//! string (`mangohud %command%`).

use std::path::{Path, PathBuf};

use super::account::{LOCALCONFIG_MAX, localconfig_path};
use super::client::client_running;
use super::keyvalues::{KvLimits, Lexer, Tok, parse, text};
use crate::error::{Error, ErrorCode};
use crate::fsx::Fs;
use crate::procfs::ProcSource;

/// The blocks above the apps' blocks.
const PATH: [&str; 5] = ["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"];
/// The key, as Steam writes it.
pub const KEY: &str = "LaunchOptions";
/// Longest value accepted, in bytes.
pub const MAX_LEN: usize = 4096;
/// The backup's file name, next to `localconfig.vdf`.
pub const BACKUP_NAME: &str = "localconfig.vdf.easyplay-bak";
/// The token that stands for the game's own command line.
pub const COMMAND: &str = "%command%";
/// MangoHud's wrapper program.
pub const MANGOHUD: &str = "mangohud";

/// Why [`set_value`] does not edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The value has a control character (a line break, a tab) or is longer than
    /// [`MAX_LEN`].
    BadValue,
    /// The file is larger than EasyPlay reads.
    TooLarge,
    /// The reader had to repair the file (cut off, a stray brace…): it is left alone.
    Damaged,
    /// The file has no `UserLocalConfigStore` block: not a `localconfig.vdf`.
    NotLocalConfig,
}

impl Refused {
    /// Text form for the error's details.
    pub const fn as_str(self) -> &'static str {
        match self {
            Refused::BadValue => "value has control characters or is too long",
            Refused::TooLarge => "file too large",
            Refused::Damaged => "file needed repairs to be read",
            Refused::NotLocalConfig => "not a localconfig.vdf",
        }
    }
}

/// What a walk over the file found.
#[derive(Debug)]
struct Found {
    /// For each level of the path (0 = the document, 1 = `UserLocalConfigStore`, …, 5 =
    /// `apps`, 6 = the app's block): where its closing `}` starts (level 0: the end).
    close: [Option<usize>; 7],
    /// The first `LaunchOptions` string of the app's block: its value token's byte range
    /// and its text.
    value: Option<(usize, usize, String)>,
    /// Structural repairs the reader would make (stray or missing braces, a key without a
    /// value, an unterminated string).
    recovered: u32,
}

fn key_is(k: &[u8], s: &str) -> bool {
    k.eq_ignore_ascii_case(s.as_bytes())
}

/// Walk the tokens of `input`, following the path to `app`'s block the way
/// [`super::keyvalues::KvObject::path`] does: keys compare case-insensitively and the first
/// object of a name is the one meant. No recursion; linear in the input.
fn walk(input: &[u8], app: u32) -> Found {
    struct Frame {
        /// Its level on the path, when it is on it.
        level: Option<usize>,
        /// Its child on the path was seen (a later one of the same name is not it).
        matched: bool,
    }
    let app_key = app.to_string();
    let seg = |level: usize| -> &str { PATH.get(level).copied().unwrap_or(app_key.as_str()) };
    let mut lx = Lexer::new(input, usize::MAX);
    let mut stack = vec![Frame {
        level: Some(0),
        matched: false,
    }];
    let mut close = [None; 7];
    close[0] = Some(input.len());
    let mut value = None;
    let mut pending: Option<Vec<u8>> = None;
    let mut recovered = 0u32;
    while let Some((tok, start, end)) = lx.next_spanned() {
        match (tok, pending.take()) {
            (Tok::Str(k), None) => pending = Some(k),
            (Tok::Str(v), Some(k)) => {
                if value.is_none()
                    && stack.last().is_some_and(|f| f.level == Some(6))
                    && key_is(&k, KEY)
                {
                    value = Some((start, end, text(v)));
                }
            }
            (Tok::Open, k) => {
                if k.is_none() {
                    recovered = recovered.saturating_add(1);
                }
                let level = match (stack.last_mut(), &k) {
                    (Some(top), Some(k)) => match top.level {
                        Some(l) if l < 6 && !top.matched && key_is(k, seg(l)) => {
                            top.matched = true;
                            Some(l + 1)
                        }
                        _ => None,
                    },
                    _ => None,
                };
                stack.push(Frame {
                    level,
                    matched: false,
                });
            }
            (Tok::Close, k) => {
                if k.is_some() {
                    recovered = recovered.saturating_add(1);
                }
                if stack.len() > 1 {
                    if let Some(Frame { level: Some(l), .. }) = stack.pop() {
                        close[l] = Some(start);
                    }
                } else {
                    recovered = recovered.saturating_add(1);
                }
            }
        }
    }
    if pending.is_some() || stack.len() > 1 {
        recovered = recovered.saturating_add(1);
    }
    Found {
        close,
        value,
        recovered: recovered.saturating_add(lx.recovered()),
    }
}

/// The launch options of `app` in `localconfig.vdf` text: `None` when the file has no value
/// for it (Steam's "none").
pub fn read_value(input: &[u8], app: u32) -> Option<String> {
    walk(input, app).value.map(|v| v.2)
}

/// Whether `value` may be written: no control characters, at most [`MAX_LEN`] bytes.
pub fn valid_value(value: &str) -> bool {
    value.len() <= MAX_LEN && !value.chars().any(char::is_control)
}

/// `s` as a quoted KeyValues string: `\` and `"` escaped (the inverse of the reader).
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if matches!(c, '\\' | '"') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The text inserted before the closing brace at `at` of the block at `level` (1–6): the
/// missing blocks below it down to the app's, and the value. Laid out like Steam lays out
/// the file — one entry per line, a tab deeper than the brace — when the brace starts its
/// line; inline otherwise. Returns the insertion point and the text.
fn insertion(input: &[u8], at: usize, level: usize, app: u32, value: &str) -> (usize, String) {
    let app_key = app.to_string();
    let segs: Vec<&str> = (level..6)
        .map(|l| PATH.get(l).copied().unwrap_or(app_key.as_str()))
        .collect();
    let line_start = input[..at]
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |i| i + 1);
    let lead = &input[line_start..at];
    if !lead.iter().all(|b| matches!(b, b' ' | b'\t')) {
        let mut out = String::from(" ");
        for s in &segs {
            out.push_str(&quote(s));
            out.push_str(" { ");
        }
        out.push_str(&quote(KEY));
        out.push(' ');
        out.push_str(&quote(value));
        out.push(' ');
        for _ in &segs {
            out.push_str("} ");
        }
        return (at, out);
    }
    let eol = if line_start >= 2 && input[line_start - 2] == b'\r' {
        "\r\n"
    } else {
        "\n"
    };
    // Only spaces and tabs: valid UTF-8.
    let mut ind = String::from_utf8_lossy(lead).into_owned();
    ind.push('\t');
    let mut out = String::new();
    for s in &segs {
        out.push_str(&format!("{ind}{}{eol}{ind}{{{eol}", quote(s)));
        ind.push('\t');
    }
    out.push_str(&format!("{ind}{}\t\t{}{eol}", quote(KEY), quote(value)));
    for _ in &segs {
        ind.pop();
        out.push_str(&format!("{ind}}}{eol}"));
    }
    (line_start, out)
}

/// `input` (a `localconfig.vdf`) with `app`'s launch options set to `value`, changing
/// nothing else: `Ok(None)` when it already says so (an absent value counts as empty). The
/// result is checked before it is returned: it reads back cleanly with `value`.
pub fn set_value(input: &[u8], app: u32, value: &str) -> Result<Option<Vec<u8>>, Refused> {
    if !valid_value(value) {
        return Err(Refused::BadValue);
    }
    if input.len() as u64 > LOCALCONFIG_MAX {
        return Err(Refused::TooLarge);
    }
    let found = walk(input, app);
    if found.recovered > 0 || parse(input, &KvLimits::LARGE).recovered > 0 {
        return Err(Refused::Damaged);
    }
    let (start, end, text) = match found.value {
        Some((_, _, cur)) if cur == value => return Ok(None),
        Some((s, e, _)) => (s, e, quote(value)),
        None if value.is_empty() => return Ok(None),
        None => {
            let (level, at) = (1..=6)
                .rev()
                .find_map(|l| found.close[l].map(|at| (l, at)))
                .ok_or(Refused::NotLocalConfig)?;
            let (at, text) = insertion(input, at, level, app, value);
            (at, at, text)
        }
    };
    let mut out = Vec::with_capacity(input.len() + text.len());
    out.extend_from_slice(&input[..start]);
    out.extend_from_slice(text.as_bytes());
    out.extend_from_slice(&input[end..]);
    let back = walk(&out, app);
    if back.recovered > 0
        || parse(&out, &KvLimits::LARGE).recovered > 0
        || back.value.map(|v| v.2).as_deref() != Some(value)
    {
        return Err(Refused::Damaged);
    }
    Ok(Some(out))
}

/// The launch options of `app` as Steam has them for the most recently used account of the
/// installation at `root`: `Some("")` when none are set, `None` when that account's
/// settings file cannot be found or read.
pub fn read(fs: &dyn Fs, root: &Path, app: u32) -> Option<String> {
    let path = localconfig_path(fs, root)?;
    let bytes = fs.read(&path, LOCALCONFIG_MAX).ok()?;
    Some(read_value(&bytes, app).unwrap_or_default())
}

/// What [`write`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    /// The file already said so: nothing was written.
    Unchanged,
    /// The file was changed; the previous one is at `backup`.
    Changed {
        /// The settings file.
        path: PathBuf,
        /// Its previous content.
        backup: PathBuf,
    },
}

fn unwritable(reason: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::StmSettingsUnwritable).with("reason", reason)
}

/// Set `app`'s launch options to `value` in the settings file of the most recently used
/// account of the installation at `root` (see the module docs). Refused with `EP-STM-008`
/// while a Steam client runs, and with `EP-STM-009` when the file cannot be found or
/// changed safely; nothing is written then.
pub fn write(
    fs: &dyn Fs,
    proc: &dyn ProcSource,
    root: &Path,
    app: u32,
    value: &str,
) -> Result<Written, Error> {
    if !valid_value(value) {
        return Err(unwritable(Refused::BadValue.as_str()));
    }
    if client_running(proc) {
        return Err(Error::new(ErrorCode::StmSteamOpen));
    }
    let path = localconfig_path(fs, root).ok_or_else(|| unwritable("no account settings"))?;
    // Filesystem errors would carry the path, which names the account: said without it.
    let fs_err = |what: &str, e: crate::fsx::FsError| unwritable(format!("{what}: {:?}", e.kind));
    let md = fs.metadata(&path).map_err(|e| fs_err("metadata", e))?;
    if !md.is_file() {
        return Err(unwritable("not a regular file"));
    }
    let before = fs
        .read(&path, LOCALCONFIG_MAX)
        .map_err(|e| fs_err("read", e))?;
    let Some(after) = set_value(&before, app, value).map_err(|r| unwritable(r.as_str()))? else {
        return Ok(Written::Unchanged);
    };
    let mode = md.mode & 0o7777;
    let backup = path.with_file_name(BACKUP_NAME);
    fs.atomic_write(&backup, &before, mode)
        .map_err(|e| fs_err("backup", e))?;
    // Steam may have started meanwhile: the last look before the file is replaced.
    if client_running(proc) {
        return Err(Error::new(ErrorCode::StmSteamOpen));
    }
    fs.atomic_write(&path, &after, mode)
        .map_err(|e| fs_err("write", e))?;
    if fs.metadata(&path).is_ok_and(|m| m.mode & 0o7777 != mode) {
        // The temporary file was created under the umask.
        fs.set_mode(&path, mode).map_err(|e| fs_err("mode", e))?;
    }
    Ok(Written::Changed { path, backup })
}

// ---- MangoHud --------------------------------------------------------------------------

/// Whitespace-separated words of `s` with their byte ranges.
fn words(s: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in s.char_indices() {
        match (c.is_whitespace(), start) {
            (true, Some(b)) => {
                out.push((b, i));
                start = None;
            }
            (false, None) => start = Some(i),
            _ => {}
        }
    }
    if let Some(b) = start {
        out.push((b, s.len()));
    }
    out
}

/// The byte range of the `mangohud` word (with the options that follow it, up to
/// `%command%`) before the first `%command%`, when there is one.
fn mangohud_span(s: &str) -> Option<(usize, usize)> {
    let cmd = s.find(COMMAND)?;
    let ws = words(s);
    let i = ws
        .iter()
        .position(|(b, e)| *e <= cmd && s[*b..*e].eq_ignore_ascii_case(MANGOHUD))?;
    let mut end = ws[i].1;
    // Its own options (`--dlsym`) go with it.
    for (b, e) in &ws[i + 1..] {
        if *e > cmd || !s[*b..*e].starts_with('-') {
            break;
        }
        end = *e;
    }
    Some((ws[i].0, end))
}

/// Whether the launch options `s` run the game under MangoHud: the word `mangohud` before
/// `%command%`. (Without `%command%`, Steam adds the text to the game's arguments, so a
/// `mangohud` there is not the overlay.)
pub fn has_mangohud(s: &str) -> bool {
    mangohud_span(s).is_some()
}

/// The launch options `s` with MangoHud on or off.
///
/// - On: `mangohud ` goes just before `%command%`; text without `%command%` (arguments
///   only, which Steam appends) becomes `mangohud %command% <text>`.
/// - Off: the `mangohud` word (and its own options) is taken out; what is left that is only
///   `%command%` followed by arguments goes back to the arguments alone.
pub fn with_mangohud(s: &str, on: bool) -> String {
    if on == has_mangohud(s) {
        return s.to_owned();
    }
    if on {
        return match s.find(COMMAND) {
            Some(i) => format!("{}{MANGOHUD} {}", &s[..i], &s[i..]),
            None if s.trim().is_empty() => format!("{MANGOHUD} {COMMAND}"),
            None => format!("{MANGOHUD} {COMMAND} {}", s.trim()),
        };
    }
    let Some((b, e)) = mangohud_span(s) else {
        return s.to_owned();
    };
    let rest = &s[e..];
    let rest = rest.trim_start();
    let out = format!("{}{rest}", &s[..b]);
    let t = out.trim();
    match t.strip_prefix(COMMAND) {
        Some(args) if args.is_empty() || args.starts_with(char::is_whitespace) => {
            args.trim().to_owned()
        }
        _ => out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const FIXTURE: &[u8] = include_bytes!("../../tests/data/steam/localconfig.vdf");

    fn set(input: &[u8], app: u32, v: &str) -> Vec<u8> {
        set_value(input, app, v).unwrap().unwrap()
    }

    #[test]
    fn reads_the_value() {
        assert_eq!(
            read_value(FIXTURE, 1_174_180).as_deref(),
            Some("PROTON_LOG=1 %command%")
        );
        assert_eq!(read_value(FIXTURE, 440), None);
        assert_eq!(read_value(FIXTURE, 999), None);
        // Escapes, case and the first of two blocks, as the reader has them.
        let doc = b"\"userlocalconfigstore\" { \"software\" { \"valve\" { \"steam\" { \"Apps\" { \
                    \"5\" { \"launchoptions\" \"a \\\"b\\\" c\\\\d\" } \"5\" { \"LaunchOptions\" \"x\" } } } } } }";
        assert_eq!(read_value(doc, 5).as_deref(), Some("a \"b\" c\\d"));
    }

    #[test]
    fn replaces_only_the_value() {
        let out = set(FIXTURE, 1_174_180, "mangohud %command% -novid");
        let old = "\"PROTON_LOG=1 %command%\"";
        let new = "\"mangohud %command% -novid\"";
        let want = String::from_utf8(FIXTURE.to_vec())
            .unwrap()
            .replace(old, new);
        assert_eq!(String::from_utf8(out.clone()).unwrap(), want);
        assert_eq!(
            read_value(&out, 1_174_180).as_deref(),
            Some("mangohud %command% -novid")
        );
        // Quotes and backslashes are escaped, and read back.
        let out = set(FIXTURE, 1_174_180, r#"-path "C:\Games" %command%"#);
        assert!(String::from_utf8_lossy(&out).contains(r#""-path \"C:\\Games\" %command%""#));
        assert_eq!(
            read_value(&out, 1_174_180).as_deref(),
            Some(r#"-path "C:\Games" %command%"#)
        );
        // Clearing keeps the key with an empty value, as Steam does.
        let out = set(FIXTURE, 1_174_180, "");
        assert!(String::from_utf8_lossy(&out).contains("\"LaunchOptions\"\t\t\"\""));
    }

    #[test]
    fn the_same_value_changes_nothing() {
        assert_eq!(
            set_value(FIXTURE, 1_174_180, "PROTON_LOG=1 %command%"),
            Ok(None)
        );
        assert_eq!(set_value(FIXTURE, 440, ""), Ok(None));
    }

    #[test]
    fn creates_the_key_in_the_apps_own_layout() {
        let out = set(FIXTURE, 440, "-novid");
        let s = String::from_utf8(out.clone()).unwrap();
        let old = "\t\t\t\t\t\t\"Playtime\"\t\t\"125\"\n\t\t\t\t\t}";
        let new = "\t\t\t\t\t\t\"Playtime\"\t\t\"125\"\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"-novid\"\n\t\t\t\t\t}";
        let want = String::from_utf8(FIXTURE.to_vec())
            .unwrap()
            .replacen(old, new, 1);
        assert_eq!(s, want);
        assert_eq!(read_value(&out, 440).as_deref(), Some("-novid"));
        assert_eq!(
            read_value(&out, 1_174_180).as_deref(),
            Some("PROTON_LOG=1 %command%")
        );
    }

    #[test]
    fn creates_a_missing_app_block_and_missing_parents() {
        let out = set(FIXTURE, 70, "%command% -console");
        let s = String::from_utf8(out.clone()).unwrap();
        assert!(
            s.contains("\t\t\t\t\t\"70\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"%command% -console\"\n\t\t\t\t\t}\n\t\t\t\t}"),
            "{s}"
        );
        assert_eq!(read_value(&out, 70).as_deref(), Some("%command% -console"));
        // Everything before the insertion point and after it is the old file.
        let at = s.find("\t\t\t\t\t\"70\"").unwrap();
        assert_eq!(&out[..at], &FIXTURE[..at]);
        assert!(out.ends_with(&FIXTURE[at..]));

        // No `apps`, no `Software` at all: the blocks are created below the top one.
        let bare = b"\"UserLocalConfigStore\"\r\n{\r\n\t\"Broadcast\"\t\t\"1\"\r\n}\r\n";
        let out = set(bare, 70, "x");
        let s = String::from_utf8(out.clone()).unwrap();
        assert_eq!(
            s,
            "\"UserLocalConfigStore\"\r\n{\r\n\t\"Broadcast\"\t\t\"1\"\r\n\
             \t\"Software\"\r\n\t{\r\n\t\t\"Valve\"\r\n\t\t{\r\n\t\t\t\"Steam\"\r\n\t\t\t{\r\n\
             \t\t\t\t\"apps\"\r\n\t\t\t\t{\r\n\t\t\t\t\t\"70\"\r\n\t\t\t\t\t{\r\n\
             \t\t\t\t\t\t\"LaunchOptions\"\t\t\"x\"\r\n\t\t\t\t\t}\r\n\t\t\t\t}\r\n\t\t\t}\r\n\
             \t\t}\r\n\t}\r\n}\r\n"
        );
        // A brace that does not start its line: inline.
        let one = b"\"UserLocalConfigStore\" { \"a\" \"1\" }";
        let out = set(one, 7, "y");
        assert_eq!(read_value(&out, 7).as_deref(), Some("y"));
        assert!(out.starts_with(b"\"UserLocalConfigStore\" { \"a\" \"1\" "));
        assert!(out.ends_with(b"}"));
    }

    #[test]
    fn refuses_what_it_cannot_edit_safely() {
        assert_eq!(set_value(FIXTURE, 440, "a\nb"), Err(Refused::BadValue));
        assert_eq!(set_value(FIXTURE, 440, "a\tb"), Err(Refused::BadValue));
        assert_eq!(
            set_value(FIXTURE, 440, &"x".repeat(MAX_LEN + 1)),
            Err(Refused::BadValue)
        );
        // Cut off mid-write.
        let cut = &FIXTURE[..FIXTURE.len() / 2];
        assert_eq!(set_value(cut, 440, "x"), Err(Refused::Damaged));
        // A stray brace.
        let mut stray = FIXTURE.to_vec();
        stray.extend_from_slice(b"}\n");
        assert_eq!(set_value(&stray, 440, "x"), Err(Refused::Damaged));
        // Not a localconfig.vdf.
        assert_eq!(
            set_value(b"\"InstallConfigStore\" { }", 440, "x"),
            Err(Refused::NotLocalConfig)
        );
    }

    #[test]
    fn mangohud_switch_cases() {
        let cases: [(&str, &str); 7] = [
            ("", "mangohud %command%"),
            ("-novid", "mangohud %command% -novid"),
            ("  -novid -console ", "mangohud %command% -novid -console"),
            ("%command%", "mangohud %command%"),
            (
                "PROTON_LOG=1 %command% -novid",
                "PROTON_LOG=1 mangohud %command% -novid",
            ),
            ("gamemoderun %command%", "gamemoderun mangohud %command%"),
            ("mangohud %command%", "mangohud %command%"),
        ];
        for (before, on) in cases {
            assert_eq!(with_mangohud(before, true), on, "on: {before:?}");
            assert!(has_mangohud(on), "{on:?}");
        }
        let offs: [(&str, &str); 7] = [
            ("mangohud %command%", ""),
            ("mangohud %command% -novid", "-novid"),
            (
                "PROTON_LOG=1 mangohud %command% -novid",
                "PROTON_LOG=1 %command% -novid",
            ),
            ("MangoHud --dlsym %command%", ""),
            ("gamemoderun mangohud %command%", "gamemoderun %command%"),
            ("-novid", "-novid"),
            ("%command% mangohud", "%command% mangohud"),
        ];
        for (before, off) in offs {
            assert_eq!(with_mangohud(before, false), off, "off: {before:?}");
            assert!(!has_mangohud(off), "{off:?}");
        }
        // `mangohud` as an argument, or inside another word, is not the overlay.
        assert!(!has_mangohud("-novid mangohud"));
        assert!(!has_mangohud("notmangohud %command%"));
        assert!(!has_mangohud("MANGOHUD=1 %command%"));
    }

    proptest! {
        /// Off after on gives back what the user wrote (trimmed when Steam would append it).
        #[test]
        fn mangohud_round_trip(words in proptest::collection::vec("[A-Za-z0-9_=%-]{1,8}", 0..5), cmd in any::<bool>()) {
            let mut s = words.join(" ");
            prop_assume!(!s.contains(COMMAND) && !s.to_lowercase().contains(MANGOHUD));
            if cmd {
                s = format!("{s} {COMMAND} -x");
            }
            let on = with_mangohud(&s, true);
            prop_assert!(has_mangohud(&on));
            let off = with_mangohud(&on, false);
            prop_assert!(!has_mangohud(&off));
            if cmd && words.is_empty() {
                // `%command% -x` and `-x` say the same to Steam; the shorter is kept.
                prop_assert_eq!(off, "-x");
            } else if cmd {
                prop_assert_eq!(off, s);
            } else {
                prop_assert_eq!(off, s.trim());
            }
        }

        /// Every byte outside the edited value is the old file's: what precedes the edit
        /// and what follows it are unchanged, and every other app's value reads the same.
        #[test]
        fn other_bytes_are_untouched(
            app in prop_oneof![Just(440u32), Just(12_210u32), Just(1_174_180u32), 1u32..100_000],
            value in "[ -~]{0,40}",
        ) {
            let Ok(Some(out)) = set_value(FIXTURE, app, &value) else {
                // Unchanged: only when it already said so.
                prop_assert_eq!(read_value(FIXTURE, app).unwrap_or_default(), value);
                return Ok(());
            };
            let pre = FIXTURE.iter().zip(&out).take_while(|(a, b)| a == b).count();
            let post = FIXTURE.iter().rev().zip(out.iter().rev()).take_while(|(a, b)| a == b).count();
            let post = post.min(FIXTURE.len().min(out.len()) - pre);
            // One contiguous change, which holds the new value.
            let changed_old = FIXTURE.len() - pre - post;
            let changed_new = &out[pre..out.len() - post];
            prop_assert!(changed_old <= "\"PROTON_LOG=1 %command%\"".len());
            prop_assert!(changed_new.len() <= quote(&value).len() + 128);
            prop_assert_eq!(read_value(&out, app).unwrap_or_default(), value);
            for other in [440u32, 12_210, 1_174_180, 7] {
                if other != app {
                    prop_assert_eq!(read_value(&out, other), read_value(FIXTURE, other));
                }
            }
            prop_assert_eq!(
                super::super::account::play_times(&out),
                super::super::account::play_times(FIXTURE)
            );
        }

        /// Every prefix and single-byte mutation of the file: never panics; edits only a
        /// file that reads cleanly, and then the value reads back.
        #[test]
        fn never_panics_on_mutations(cut in 0usize..400, byte in any::<u8>(), at in 0usize..400) {
            let mut m = FIXTURE[..cut.min(FIXTURE.len())].to_vec();
            if !m.is_empty() {
                let i = at % m.len();
                m[i] = byte;
            }
            if let Ok(Some(out)) = set_value(&m, 440, "v") {
                prop_assert_eq!(read_value(&out, 440), Some("v".to_owned()));
            }
        }
    }
}
