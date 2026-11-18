//! Display names from PE strings, folder and file names (GAME_IDENTITY §7).

use std::path::Path;

use crate::consts::GAME_NAME_MAX_CHARS;
use crate::model::PeInfo;

use super::game_root::is_binary_dir_name;

/// Product or description values that do not name a game.
const GENERIC: &[&str] = &[
    "setup",
    "installer",
    "launcher",
    "application",
    "game",
    "unity",
    "unreal",
    "ue4game",
    "ue5game",
    "bootstrap",
];

/// Trailing tokens removed by cleaning.
const JUNK_TAIL: &[&str] = &[
    "x64", "x86", "win64", "win32", "shipping", "final", "release", "launcher",
];

/// Whether a cleaned candidate is generic (the rule list of §7, plus "equal to the
/// company", which catches engine and vendor names).
pub fn is_generic(cleaned: &str, company: Option<&str>) -> bool {
    let l = cleaned.to_lowercase();
    if l.is_empty() || GENERIC.contains(&l.as_str()) || l.starts_with("crash") {
        return true;
    }
    company.is_some_and(|c| clean(c).to_lowercase() == l)
}

fn is_version_token(t: &str) -> bool {
    let t = t.to_lowercase();
    let body = t.strip_prefix('v').unwrap_or(&t);
    let has_v = body.len() != t.len();
    if body.is_empty() || !body.starts_with(|c: char| c.is_ascii_digit()) {
        return false;
    }
    let body = body
        .strip_suffix(|c: char| c.is_ascii_lowercase())
        .unwrap_or(body);
    let ok = body.chars().all(|c| c.is_ascii_digit() || c == '.')
        && !body.ends_with('.')
        && !body.contains("..");
    ok && (has_v || body.contains('.'))
}

/// Split off the last token (separated by whitespace or `-`). Returns `(head, token)`.
fn last_token(s: &str) -> (&str, &str) {
    match s.rfind(|c: char| c.is_whitespace() || c == '-') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => ("", s),
    }
}

fn strip_tails(mut s: String) -> String {
    loop {
        let t = s.trim_end_matches(|c: char| c.is_whitespace() || c == '-');
        let before = t.len();
        let mut next = t.to_owned();
        let lower = t.to_lowercase();
        if lower.ends_with("-windows") {
            next.truncate(t.len() - "-windows".len());
        } else if let Some(open) = t.rfind('(')
            && t.ends_with(')')
            && t[open + 1..t.len() - 1].chars().all(|c| c.is_ascii_digit())
            && t.len() - open > 2
            && open > 0
        {
            next.truncate(open);
        } else {
            let (head, tok) = last_token(t);
            let tl = tok.to_lowercase();
            if !head.trim().is_empty() {
                if JUNK_TAIL.contains(&tl.as_str()) || is_version_token(tok) {
                    next = head.to_owned();
                } else if tok.chars().all(|c| c.is_ascii_digit()) && !tok.is_empty() {
                    let (h2, prev) = last_token(head.trim_end());
                    if prev.eq_ignore_ascii_case("build") && !h2.trim().is_empty() {
                        next = h2.to_owned();
                    }
                }
            }
        }
        let next = next
            .trim_end_matches(|c: char| c.is_whitespace() || c == '-')
            .to_owned();
        if next.len() == before || next.is_empty() {
            return if next.is_empty() { t.to_owned() } else { next };
        }
        s = next;
    }
}

fn split_camel(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 {
            let p = chars[i - 1];
            let n = chars.get(i + 1).copied();
            let boundary = (p.is_lowercase() && c.is_uppercase())
                || (p.is_uppercase() && c.is_uppercase() && n.is_some_and(char::is_lowercase))
                || (p.is_alphabetic() && c.is_ascii_digit())
                || (p.is_ascii_digit() && c.is_alphabetic());
            if boundary {
                out.push(' ');
            }
        }
        out.push(c);
    }
    out
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The cleaning rules of §7.
pub fn clean(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut s = String::with_capacity(raw.len());
    for (i, &c) in chars.iter().enumerate() {
        let digit = |j: Option<usize>| {
            j.and_then(|j| chars.get(j))
                .is_some_and(|c| c.is_ascii_digit())
        };
        match c {
            '_' => s.push(' '),
            '.' if !(digit(i.checked_sub(1)) && digit(Some(i + 1))) => s.push(' '),
            c if c.is_control() => {}
            c => s.push(c),
        }
    }
    let mut s = strip_tails(collapse(&s));
    if !s.contains(' ') {
        s = strip_tails(collapse(&split_camel(&s)));
    }
    let s = collapse(&s);
    s.chars()
        .take(GAME_NAME_MAX_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The display name of a new game: first non-empty candidate of §7.
pub fn derive_name(pe: &PeInfo, root: Option<&Path>, exe: &Path) -> String {
    let company = pe.company.as_deref();
    for cand in [&pe.product_name, &pe.file_description]
        .into_iter()
        .flatten()
    {
        let c = clean(cand);
        if !is_generic(&c, company) {
            return c;
        }
    }
    if let Some(dir) = root.and_then(Path::file_name).map(|n| n.to_string_lossy())
        && !is_binary_dir_name(&dir)
    {
        let c = clean(&dir);
        if !c.is_empty() {
            return c;
        }
    }
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let c = clean(&stem);
    if c.is_empty() { "Game".to_owned() } else { c }
}

/// The sort key: case-folded, with a leading English article moved to the end
/// ("The Witcher 3" → "witcher 3, the").
pub fn sort_name(name: &str) -> String {
    let l = name.trim().to_lowercase();
    for art in ["the ", "a ", "an "] {
        if let Some(rest) = l.strip_prefix(art)
            && !rest.trim().is_empty()
        {
            return format!("{}, {}", rest.trim(), art.trim());
        }
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleaning_table() {
        for (raw, want) in [
            ("Hollow_Knight", "Hollow Knight"),
            ("HollowKnight", "Hollow Knight"),
            ("Portal2", "Portal 2"),
            ("Portal 2", "Portal 2"),
            ("Fallout 76", "Fallout 76"),
            ("Game-Win64-Shipping", "Game"),
            ("Stray-Win64-Shipping", "Stray"),
            ("MyGame x64", "My Game"),
            ("Celeste v1.4.0.0", "Celeste"),
            ("Celeste 1.0.4b", "Celeste"),
            ("Terraria build 1234", "Terraria"),
            ("Terraria (1)", "Terraria"),
            ("Half.Life.2", "Half Life 2"),
            ("witcher3", "witcher 3"),
            ("XMLGame", "XML Game"),
            ("Game-windows", "Game"),
            ("Some Game Launcher", "Some Game"),
            ("Launcher", "Launcher"),
            ("  Spaced   Out  ", "Spaced Out"),
            ("RDR2", "RDR 2"),
            ("eldenring", "eldenring"),
            ("Factorio Final Release", "Factorio"),
        ] {
            assert_eq!(clean(raw), want, "{raw:?}");
        }
    }

    #[test]
    fn version_tokens() {
        assert!(is_version_token("v1"));
        assert!(is_version_token("1.2.3"));
        assert!(is_version_token("1.0.4b"));
        assert!(!is_version_token("2"));
        assert!(!is_version_token("76"));
        assert!(!is_version_token("v"));
        assert!(!is_version_token("vault"));
    }

    #[test]
    fn candidates_in_order() {
        let exe = Path::new("/g/Hollow Knight/hollow_knight.exe");
        let root = Some(Path::new("/g/Hollow Knight"));
        let mut pe = PeInfo {
            product_name: Some("Hollow Knight".into()),
            ..PeInfo::default()
        };
        assert_eq!(derive_name(&pe, root, exe), "Hollow Knight");
        pe.product_name = Some("Unity".into());
        pe.file_description = Some("Unity Player".into());
        pe.company = Some("Unity Player".into());
        assert_eq!(derive_name(&pe, root, exe), "Hollow Knight");
        let bin = Some(Path::new("/g/X/bin"));
        assert_eq!(
            derive_name(&PeInfo::default(), bin, Path::new("/g/X/bin/CoolGame.exe")),
            "Cool Game"
        );
        assert_eq!(
            derive_name(&PeInfo::default(), None, Path::new("/d/CrashGame_x64.exe")),
            "Crash Game"
        );
        let generic = PeInfo {
            product_name: Some("CrashReporter".into()),
            ..PeInfo::default()
        };
        assert_eq!(derive_name(&generic, None, Path::new("/d/Foo.exe")), "Foo");
    }

    #[test]
    fn sort_names() {
        assert_eq!(sort_name("The Witcher 3"), "witcher 3, the");
        assert_eq!(sort_name("A Plague Tale"), "plague tale, a");
        assert_eq!(sort_name("An Untitled Story"), "untitled story, an");
        assert_eq!(sort_name("Theatre"), "theatre");
        assert_eq!(sort_name("The"), "the");
    }
}
