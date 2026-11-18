//! Rendering `.desktop` files (DESKTOP_INTEGRATION §2, §6).
//!
//! Only per-game shortcuts are rendered at run time; the application's own entry is a static
//! file under `assets/share/applications`. `Exec` is assembled from fixed parts and a
//! [`GameId`] only, so no user-controlled text can reach it. `Name` goes through
//! [`escape_value`].

use crate::ids::GameId;

/// The program name in `Exec` lines (resolved through `PATH` by the desktop).
pub const PROGRAM: &str = "zaun";
/// The key that marks a shortcut as Zaun's and names its game.
pub const GAME_ID_KEY: &str = "X-Zaun-GameId";
/// File-name prefix of per-game menu entries and exported icons.
pub const SHORTCUT_PREFIX: &str = "zaun-game-";
/// Longest `Name` written (characters); longer names are cut.
pub const NAME_MAX_CHARS: usize = 200;

/// A per-game shortcut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameEntry<'a> {
    /// The game.
    pub id: GameId,
    /// Its display name (any text).
    pub name: &'a str,
    /// Icon name in the icon theme (`None`: the application icon).
    pub icon: Option<String>,
}

/// The icon name of a game's exported icon.
pub fn icon_name(id: GameId) -> String {
    format!("{SHORTCUT_PREFIX}{id}")
}

/// The menu entry's file name (`zaun-game-<id>.desktop`).
pub fn file_name(id: GameId) -> String {
    format!("{SHORTCUT_PREFIX}{id}.desktop")
}

/// Make arbitrary text a valid single-line `localestring` value: control characters removed
/// (newlines and tabs become spaces), Unicode direction overrides removed, the escapes of the
/// Desktop Entry Specification applied (`\\`, `\s` for a leading space), cut to
/// [`NAME_MAX_CHARS`]. An empty result becomes `"Game"`.
pub fn escape_value(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    for c in text.chars().take(NAME_MAX_CHARS * 2) {
        match c {
            '\n' | '\r' | '\t' => clean.push(' '),
            c if c.is_control() => {}
            // Bidirectional overrides and isolates (RTL tricks in file managers).
            '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' => {}
            c => clean.push(c),
        }
    }
    let clean: String = clean.trim_end().chars().take(NAME_MAX_CHARS).collect();
    if clean.trim().is_empty() {
        return "Game".to_owned();
    }
    let mut out = String::with_capacity(clean.len() + 4);
    for (i, c) in clean.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            ' ' if i == 0 => out.push_str("\\s"),
            c => out.push(c),
        }
    }
    out
}

/// The `Exec` line of a game's shortcut: only the program, the fixed verb and the ID.
pub fn exec_line(id: GameId) -> String {
    format!("{PROGRAM} launch {id}")
}

/// Render a game's menu entry (DESKTOP_INTEGRATION §6).
pub fn render(entry: &GameEntry<'_>) -> String {
    let icon = entry
        .icon
        .as_deref()
        .filter(|i| {
            !i.is_empty()
                && i.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
        .unwrap_or(crate::consts::APP_ID);
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Version=1.5\n\
         Name={name}\n\
         Comment=Play with Zaun\n\
         Exec={exec}\n\
         TryExec={PROGRAM}\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=Game;\n\
         StartupNotify=false\n\
         {GAME_ID_KEY}={id}\n",
        name = escape_value(entry.name),
        exec = exec_line(entry.id),
        id = entry.id,
    )
}

/// Read the value of `key` in the `[Desktop Entry]` group of a desktop file (no locale
/// variants, escapes undone for `\s` and `\\`).
pub fn read_key(text: &str, key: &str) -> Option<String> {
    let mut in_main = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        if let Some((k, v)) = line.split_once('=')
            && k.trim_end() == key
        {
            return Some(unescape(v.trim_start()));
        }
    }
    None
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// A file name for the copy on the Desktop: the name reduced to safe characters.
pub fn desktop_file_name(name: &str, id: GameId) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    let trimmed = s
        .trim_matches(|c: char| matches!(c, '.' | ' ' | '-' | '_'))
        .to_owned();
    s = trimmed;
    if s.is_empty() {
        s = file_name(id).trim_end_matches(".desktop").to_owned();
    }
    format!("{s}.desktop")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> GameId {
        "0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5e".parse().unwrap()
    }

    #[test]
    fn hostile_names_never_reach_exec() {
        for name in [
            "Hollow Knight",
            "Evil\nExec=rm -rf ~",
            "50% off %f %U",
            "a;b=c",
            "-flag",
            " leading space",
            "back\\slash",
            "\u{202E}exe.txt",
            &"x".repeat(5000),
            "",
            "\t\r\n",
        ] {
            let text = render(&GameEntry {
                id: id(),
                name,
                icon: Some(icon_name(id())),
            });
            let lines: Vec<&str> = text.lines().collect();
            assert_eq!(
                lines.iter().filter(|l| l.starts_with("Exec=")).count(),
                1,
                "{name:?}"
            );
            assert!(lines.contains(&"Exec=zaun launch 0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5e"));
            assert_eq!(lines.len(), 12, "{name:?}: {text}");
            let n = read_key(&text, "Name").unwrap();
            assert!(!n.is_empty());
            assert!(n.chars().count() <= NAME_MAX_CHARS);
            assert!(!n.contains('\u{202E}'));
            assert_eq!(read_key(&text, GAME_ID_KEY).unwrap(), id().to_string());
        }
    }

    #[test]
    fn escapes_round_trip() {
        assert_eq!(escape_value(" a\\b"), "\\sa\\\\b");
        assert_eq!(unescape(&escape_value(" a\\b")), " a\\b");
        assert_eq!(escape_value("a\nb"), "a b");
    }

    #[test]
    fn icon_names_are_restricted() {
        let t = render(&GameEntry {
            id: id(),
            name: "x",
            icon: Some("../../etc/passwd".into()),
        });
        assert!(t.contains("Icon=io.github.zaun.Zaun\n"));
    }

    #[test]
    fn desktop_copy_names() {
        assert_eq!(
            desktop_file_name("Hollow Knight", id()),
            "Hollow Knight.desktop"
        );
        assert_eq!(desktop_file_name("../x/y", id()), "x_y.desktop");
        assert_eq!(
            desktop_file_name("...", id()),
            "zaun-game-0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5e.desktop"
        );
    }
}
