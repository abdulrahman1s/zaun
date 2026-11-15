//! What a line of a session log is (ADR-0025): which program wrote it and how serious it is.
//!
//! `game.log` interleaves umu, pressure-vessel, Proton and its game fixes, Wine and the game,
//! each prefixed by the supervisor with the time since start (`[+1.234] `). The Proton log
//! (`PROTON_LOG=1`) is Wine's debug output. Each of them marks its lines in its own way;
//! [`classify`] recognises those marks. A line nobody marked is the game's, at level info.
//! Pure text in, two small enums out: the lines are untrusted and only ever compared.

/// How serious a line is, most serious first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LineLevel {
    /// Something failed (`err:`, `ERROR`, `E:`, `CRITICAL`, a crash report).
    Error,
    /// Something may be wrong (`warn:`, `WARN`, `W:`).
    Warning,
    /// Ordinary output.
    #[default]
    Info,
    /// Detail for debugging (`fixme:`, `trace:`, `DEBUG`, `D:`).
    Debug,
}

impl LineLevel {
    /// Every level, most serious first.
    pub const ALL: [LineLevel; 4] = [Self::Error, Self::Warning, Self::Info, Self::Debug];
}

/// Who wrote a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineSource {
    /// Wine: its debug channels, its messages, and libraries' messages from a Wine process.
    Wine,
    /// Proton, its game fixes, and the Steam Runtime container (pressure-vessel).
    Proton,
    /// umu-launcher.
    Umu,
    /// EasyPlay's own notes in the log (`[… similar lines hidden …]`).
    EasyPlay,
    /// The game, and anything else that printed.
    #[default]
    Game,
}

/// The source and level of one line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LineKind {
    /// Who wrote it.
    pub source: LineSource,
    /// How serious it is.
    pub level: LineLevel,
}

/// The line without the supervisor's `[+12.345] ` time prefix.
pub fn without_time(line: &str) -> &str {
    line.strip_prefix("[+")
        .and_then(|rest| {
            let end = rest.find("] ")?;
            rest[..end]
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'.')
                .then(|| &rest[end + 2..])
        })
        .unwrap_or(line)
}

fn level_word(word: &str) -> Option<LineLevel> {
    Some(match word {
        "ERROR" | "ERR" | "CRITICAL" | "FATAL" | "E" => LineLevel::Error,
        "WARNING" | "WARN" | "W" => LineLevel::Warning,
        "INFO" | "MESSAGE" | "I" | "NOTICE" => LineLevel::Info,
        "DEBUG" | "TRACE" | "D" => LineLevel::Debug,
        _ => return None,
    })
}

/// A Wine debug channel line: `[pid:tid:]class:channel:function …`, class one of
/// `err`, `warn`, `fixme`, `trace` (with or without Wine's `+pid`/`+tid`/timestamp fields).
fn wine_channel(text: &str) -> Option<LineLevel> {
    let mut rest = text;
    // Optional `12345.678:` timestamp and `0024:` / `0024:0028:` thread fields.
    for _ in 0..3 {
        match rest.split_once(':') {
            Some((head, tail))
                if !head.is_empty()
                    && head.len() <= 16
                    && head.bytes().all(|b| b.is_ascii_hexdigit() || b == b'.') =>
            {
                rest = tail;
            }
            _ => break,
        }
    }
    let (class, tail) = rest.split_once(':')?;
    // `class:channel:` — the channel is a short identifier.
    let channel = tail.split(':').next()?;
    if channel.is_empty()
        || channel.len() > 32
        || !channel
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return None;
    }
    Some(match class {
        "err" => LineLevel::Error,
        "warn" => LineLevel::Warning,
        "fixme" | "trace" => LineLevel::Debug,
        _ => return None,
    })
}

/// A GLib message: `Domain-LEVEL **: …` (`GStreamer-WARNING **:`), after `(prog:pid): `.
fn glib_level(text: &str) -> Option<LineLevel> {
    let head = text.split(" **").next()?;
    if head.len() == text.len() {
        return None;
    }
    let word = head.rsplit('-').next()?;
    level_word(word).or(match word {
        "Message" => Some(LineLevel::Info),
        _ => None,
    })
}

/// `Name[123]: X: …` or `Name[123] X: …`: a program name with a PID, then a level word.
fn tagged(text: &str) -> Option<(&str, Option<LineLevel>)> {
    let open = text.find('[')?;
    let close = open + text[open..].find(']')?;
    let name = &text[..open];
    if name.is_empty()
        || name.contains(' ')
        || !text[open + 1..close].bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let rest = text[close + 1..].trim_start_matches(':').trim_start();
    let level = rest.split_once(':').and_then(|(w, _)| level_word(w.trim()));
    Some((name, level))
}

/// The source and level of `line` (with or without the time prefix).
pub fn classify(line: &str) -> LineKind {
    let text = without_time(line).trim_start();
    let kind = |source, level| LineKind { source, level };
    if text.starts_with('[') && text.contains("similar lines hidden") {
        return kind(LineSource::EasyPlay, LineLevel::Info);
    }
    if let Some((name, level)) = tagged(text) {
        let source = match name {
            "ProtonFixes"
            | "pressure-vessel-wrap"
            | "pressure-vessel-adverb"
            | "pressure-vessel-locale-gen"
            | "steam-runtime-launch-client" => Some(LineSource::Proton),
            n if n.starts_with("pressure-vessel") || n.starts_with("steam-runtime") => {
                Some(LineSource::Proton)
            }
            _ => None,
        };
        if let Some(source) = source {
            return kind(source, level.unwrap_or(LineLevel::Info));
        }
    }
    // umu: `INFO: …`, `WARNING: …`, `[umu.umu_run:123] DEBUG: …`.
    let umu = text
        .strip_prefix("[umu")
        .and_then(|r| r.split_once("] ").map(|(_, t)| t))
        .unwrap_or(text);
    if let Some((word, _)) = umu.split_once(": ")
        && let Some(level) = level_word(word)
        && word.len() > 1
    {
        return kind(LineSource::Umu, level);
    }
    if let Some(level) = wine_channel(text) {
        return kind(LineSource::Wine, level);
    }
    if let Some(rest) = text
        .strip_prefix("(wine:")
        .or_else(|| text.strip_prefix("(wine64:"))
    {
        let rest = rest.split_once("): ").map_or(rest, |(_, t)| t);
        return kind(
            LineSource::Wine,
            glib_level(rest).unwrap_or(LineLevel::Info),
        );
    }
    if text.starts_with("wine: ") || text.starts_with("wineserver: ") {
        let lower = text.to_ascii_lowercase();
        let level = if ["unhandled", "could not", "failed", "error", "crash"]
            .iter()
            .any(|w| lower.contains(w))
        {
            LineLevel::Error
        } else {
            LineLevel::Info
        };
        return kind(LineSource::Wine, level);
    }
    if text.starts_with("Proton:")
        || text.starts_with("fsync:")
        || text.starts_with("esync:")
        || text.starts_with("ntsync:")
    {
        return kind(LineSource::Proton, LineLevel::Info);
    }
    if let Some(level) = glib_level(text) {
        return kind(LineSource::Game, level);
    }
    // Crash reports and plain failures of the game or a library.
    let lower = text.to_ascii_lowercase();
    let level = if lower.starts_with("error")
        || lower.contains("segmentation fault")
        || lower.contains("unhandled exception")
        || lower.starts_with("fatal")
        || lower.starts_with("traceback (most recent call last)")
    {
        LineLevel::Error
    } else if lower.starts_with("warning") {
        LineLevel::Warning
    } else {
        LineLevel::Info
    };
    kind(LineSource::Game, level)
}

#[cfg(test)]
mod tests {
    use super::*;
    use LineLevel::*;
    use LineSource::*;

    fn k(line: &str) -> (LineSource, LineLevel) {
        let c = classify(line);
        (c.source, c.level)
    }

    #[test]
    fn real_lines_of_a_session_log() {
        let cases = [
            ("[+0.284] INFO: umu-launcher version 1.4.4", (Umu, Info)),
            ("[+0.3] WARNING: Proton not found", (Umu, Warning)),
            ("[umu.umu_run:441] DEBUG: Ignoring env", (Umu, Debug)),
            ("ERROR: Failed to download runtime", (Umu, Error)),
            (
                "[+2.450] pressure-vessel-wrap[39]: W: \"/x\" is unlikely to appear",
                (Proton, Warning),
            ),
            (
                "pressure-vessel-wrap[39]: E: Unable to run",
                (Proton, Error),
            ),
            (
                "[+2.929] ProtonFixes[173] INFO: Running checks",
                (Proton, Info),
            ),
            (
                "ProtonFixes[173] WARN: Game title not found in CSV",
                (Proton, Warning),
            ),
            ("[+3.213] Proton: Executable a unix path", (Proton, Info)),
            ("[+3.220] fsync: up and running.", (Proton, Info)),
            (
                "[+31.209] (wine:252): GStreamer-WARNING **: 12:35:08.223: Failed to load plugin",
                (Wine, Warning),
            ),
            (
                "(wine:252): GStreamer-Video-CRITICAL **: 12:35:08.575: assertion failed",
                (Wine, Error),
            ),
            (
                "0024:err:module:import_dll Library d3dx9_43.dll not found",
                (Wine, Error),
            ),
            (
                "0024:fixme:ntdll:NtQuerySystemInformation info_class",
                (Wine, Debug),
            ),
            (
                "012c:0130:warn:seh:dispatch_exception code=c0000005",
                (Wine, Warning),
            ),
            (
                "4521.123:0024:0028:trace:loaddll:build_module Loaded",
                (Wine, Debug),
            ),
            (
                "err:winediag:nodrv_CreateWindow Application tried",
                (Wine, Error),
            ),
            ("wine: Unhandled page fault on read access", (Wine, Error)),
            (
                "[+306.205] [24672 similar lines hidden: hard-link fallback]",
                (EasyPlay, Info),
            ),
            (
                "ALSA lib seq_hw.c:466:(snd_seq_hw_open) open /dev/snd/seq failed",
                (Game, Info),
            ),
            ("Segmentation fault (core dumped)", (Game, Error)),
            ("Loading level 3", (Game, Info)),
            ("", (Game, Info)),
        ];
        for (line, want) in cases {
            assert_eq!(k(line), want, "{line}");
        }
    }

    #[test]
    fn the_time_prefix_is_only_the_supervisors() {
        assert_eq!(without_time("[+1.5] x"), "x");
        assert_eq!(without_time("[+a] x"), "[+a] x");
        assert_eq!(without_time("[1.5] x"), "[1.5] x");
        // Not a Wine channel: a class without a channel, or a sentence with colons.
        assert_eq!(k("err: something"), (Game, Info));
        assert_eq!(k("Note: fixme: later"), (Game, Info));
    }

    #[test]
    fn levels_are_ordered_by_seriousness() {
        assert!(Error < Warning && Warning < Info && Info < Debug);
    }
}
