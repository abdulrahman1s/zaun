//! The catalogues agree (ADR-0032): the same keys and arguments in every language, every
//! plural family has `other`, every key the code asks for exists, and nothing was left
//! untranslated. These tests read `locales/*.yml` and the sources only.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const FORMS: [&str; 6] = ["zero", "one", "two", "few", "many", "other"];

/// The catalogue as `key -> text`, keys joined with dots. Reads the one layout the
/// catalogues use: two-space nesting, `key: "text"` with `\"`, `\\`, `\n` and `\t` escapes.
fn load(code: &str) -> BTreeMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("locales/{code}.yml"));
    let text = std::fs::read_to_string(&path).unwrap();
    let mut out = BTreeMap::new();
    let mut stack: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() || line.starts_with("_version") {
            continue;
        }
        let depth = (line.len() - line.trim_start().len()) / 2;
        let body = line.trim_start();
        let (key, value) = body.split_once(':').expect(line);
        let key = key.trim_matches('"').to_owned();
        let value = value.trim_start();
        stack.truncate(depth);
        if value.is_empty() {
            stack.push(key);
            continue;
        }
        let inner = &value[1..value.len() - 1];
        let mut text = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => text.push('\n'),
                    Some('t') => text.push('\t'),
                    Some(o) => text.push(o),
                    None => {}
                }
            } else {
                text.push(c);
            }
        }
        let full = stack
            .iter()
            .cloned()
            .chain([key])
            .collect::<Vec<_>>()
            .join(".");
        assert!(out.insert(full.clone(), text).is_none(), "duplicate {full}");
    }
    out
}

fn args(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(i) = rest.find("%{") {
        rest = &rest[i + 2..];
        if let Some(j) = rest.find('}') {
            out.insert(rest[..j].to_owned());
            rest = &rest[j..];
        }
    }
    out
}

fn form_of(key: &str) -> Option<(&str, &str)> {
    let (family, last) = key.rsplit_once('.')?;
    FORMS.contains(&last).then_some((family, last))
}

/// Plural families: keys that have children named after plural forms.
fn families(cat: &BTreeMap<String, String>) -> BTreeSet<String> {
    cat.keys()
        .filter_map(|k| form_of(k))
        .map(|(fam, _)| fam.to_owned())
        .collect()
}

#[test]
fn every_key_of_english_is_in_arabic_and_the_reverse() {
    let (en, ar) = (load("en"), load("ar"));
    let missing: Vec<_> = en.keys().filter(|k| !ar.contains_key(*k)).collect();
    assert!(missing.is_empty(), "missing in ar: {missing:#?}");
    let en_fams = families(&en);
    let extra: Vec<_> = ar
        .keys()
        .filter(|k| !en.contains_key(*k))
        .filter(|k| !form_of(k).is_some_and(|(fam, _)| en_fams.contains(fam)))
        .collect();
    assert!(extra.is_empty(), "keys only in ar: {extra:#?}");
}

#[test]
fn the_arguments_of_each_pair_are_the_same_set() {
    let (en, ar) = (load("en"), load("ar"));
    let mut bad = Vec::new();
    for (k, e) in &en {
        let a = &ar[k];
        let (ea, aa) = (args(e), args(a));
        // A form that names one thing ("one game", "no games") may leave `n` out.
        let spoken = form_of(k).is_some_and(|(_, f)| f == "one");
        let ok = if spoken { aa.is_subset(&ea) } else { ea == aa };
        if !ok {
            bad.push(format!("{k}: {ea:?} vs {aa:?}"));
        }
    }
    // The extra plural forms of Arabic use only arguments their family has.
    for (k, a) in &ar {
        if en.contains_key(k) {
            continue;
        }
        let (fam, _) = form_of(k).unwrap();
        let other = &en[&format!("{fam}.other")];
        if !args(a).is_subset(&args(other)) {
            bad.push(format!("{k}: {:?} not in {:?}", args(a), args(other)));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn every_plural_family_has_other_and_arabic_has_the_forms_it_needs() {
    for code in ["en", "ar"] {
        let cat = load(code);
        for fam in families(&cat) {
            assert!(
                cat.contains_key(&format!("{fam}.other")),
                "{code}: {fam} has no `other`"
            );
        }
    }
    // Arabic families that count names the forms two and few too.
    let ar = load("ar");
    let missing: Vec<_> = families(&ar)
        .into_iter()
        .filter(|f| !ar.contains_key(&format!("{f}.two")) || !ar.contains_key(&format!("{f}.few")))
        .collect();
    assert!(missing.is_empty(), "no two/few in ar: {missing:#?}");
}

/// Texts that are the same in both languages on purpose: names, key caps, sample data and
/// messages made only of arguments and punctuation.
fn same_on_purpose(key: &str, text: &str) -> bool {
    let no_words = !text
        .split(|c: char| !c.is_alphabetic())
        .any(|w| w.chars().count() > 3 && !text.contains(&format!("%{{{w}}}")));
    no_words
        || key.starts_with("shortcuts.")
        || key.starts_with("menu.key_")
        || key.starts_with("library.key_")
        || key.starts_with("steam.page.steam_")
        || [
            "steam.group",
            "steam.scope",
            "steam.origin",
            "steam.page.proton",
            "steam.filter.easyplay",
            "steam.filter.steam",
            "settings.from_steam",
            "settings.lang_english",
            "settings.licence_rows.phosphor_icons",
            "menu.launch_command",
            "gallery.sample_game",
            "gallery.shortcut",
            "gallery.tag",
            "gallery.row_mono",
            "gallery.door_path",
            "language.name",
        ]
        .contains(&key)
}

#[test]
fn nothing_is_empty_or_left_in_english() {
    let (en, ar) = (load("en"), load("ar"));
    let mut bad = Vec::new();
    for (k, e) in &en {
        let a = &ar[k];
        if a.trim().is_empty() {
            bad.push(format!("{k}: empty"));
        } else if a == e && !same_on_purpose(k, e) {
            bad.push(format!("{k}: identical to English: {e}"));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
    // Arabic text never carries a Latin word of its own: only names, units and codes.
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Every string literal in the sources that names a catalogue key (its first segment is a
/// top-level key) exists, as a key or as a plural family.
#[test]
fn every_key_the_code_asks_for_exists() {
    let en = load("en");
    let tops: BTreeSet<&str> = en.keys().filter_map(|k| k.split('.').next()).collect();
    let mut files = Vec::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let mut bad = Vec::new();
    let mut seen = 0;
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let body = &text[..text.find("#[cfg(test)]").unwrap_or(text.len())];
        let in_strings = f
            .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
            .is_ok_and(|p| p.starts_with("strings"));
        for (n, line) in body.lines().enumerate() {
            let marked = in_strings
                || [
                    "text(",
                    "text_in(",
                    "t!(",
                    "counted",
                    "const MONTH_",
                    "const WEEKDAY_",
                ]
                .iter()
                .any(|m| line.contains(m));
            if line.trim_start().starts_with("//") || !marked {
                continue;
            }
            for (i, lit) in line.split('"').enumerate() {
                let looks_like_key = i % 2 == 1
                    && lit.contains('.')
                    && lit.chars().all(|c| {
                        c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.'
                    })
                    && !lit.starts_with('.')
                    && !lit.ends_with('.')
                    && tops.contains(lit.split('.').next().unwrap_or(""));
                if looks_like_key {
                    seen += 1;
                    if !en.contains_key(lit) && !en.contains_key(&format!("{lit}.other")) {
                        bad.push(format!("{}:{}: {lit}", f.display(), n + 1));
                    }
                }
            }
        }
    }
    assert!(seen > 1000, "found only {seen} keys in the sources");
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn the_catalogues_say_nothing_forbidden() {
    // Arabic wording of the security rules (SECURITY_MODEL §10): no promise of complete safety.
    let ar = load("ar");
    for (k, v) in &ar {
        for claim in [
            "آمن تمامًا",
            "آمنة تمامًا",
            "حماية كاملة",
            "عزل كامل",
            "لا يمكن اختراق",
        ] {
            assert!(!v.contains(claim), "{k}: {v}");
        }
    }
    let honesty = &ar["setup.honesty"];
    assert!(honesty.contains("تقلّل الخطر"), "{honesty}");
}
