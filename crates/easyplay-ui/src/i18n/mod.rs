//! Languages of the interface (ADR-0032): the catalogue lookup, the language setting, and
//! the pieces right-to-left text needs ([`plural`] categories, [`bidi`] reordering).
//!
//! Translations are compiled in from `locales/*.yml` by `rust_i18n::i18n!` in `lib.rs`;
//! nothing is read from disk at run time. The current language is process-wide.

pub mod bidi;
pub mod plural;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use rust_i18n::t;

/// A language the interface ships in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Language {
    /// English: the source language and the fallback.
    English,
    /// Arabic (Modern Standard), written right to left.
    Arabic,
}

/// The reading direction of a language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    /// Left to right.
    Ltr,
    /// Right to left.
    Rtl,
}

/// The `general.language` preference: follow the desktop, or a fixed language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LanguageChoice {
    /// "system": the desktop's language when shipped, else English.
    System,
    /// A fixed language.
    Is(Language),
}

impl Language {
    /// Every shipped language, English first.
    pub const ALL: [Language; 2] = [Language::English, Language::Arabic];

    /// The catalogue code: "en" or "ar".
    pub const fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Arabic => "ar",
        }
    }

    /// The language with this code (case-insensitive), if shipped.
    pub fn parse(code: &str) -> Option<Language> {
        let code = code.trim();
        Language::ALL
            .into_iter()
            .find(|l| l.code().eq_ignore_ascii_case(code))
    }

    /// The language's reading direction.
    pub const fn dir(self) -> Dir {
        match self {
            Language::English => Dir::Ltr,
            Language::Arabic => Dir::Rtl,
        }
    }

    /// The language's name in itself.
    pub fn name(self) -> &'static str {
        text_in(self, "language.name")
    }
}

impl LanguageChoice {
    /// The config value: "system", "en" or "ar".
    pub const fn value(self) -> &'static str {
        match self {
            LanguageChoice::System => "system",
            LanguageChoice::Is(l) => l.code(),
        }
    }

    /// Reads the config value (a tag such as "ar-SA" counts as its language part);
    /// anything unknown is [`LanguageChoice::System`].
    pub fn parse(value: &str) -> LanguageChoice {
        let lang = value.split(['_', '-']).next().unwrap_or_default();
        Language::parse(lang).map_or(LanguageChoice::System, LanguageChoice::Is)
    }
}

/// Resolves the choice against the desktop's locale ("ar_EG.UTF-8", "ar", "en_GB"...):
/// its language part if we ship it, else English.
pub fn resolve(choice: LanguageChoice, system_locale: Option<&str>) -> Language {
    match choice {
        LanguageChoice::Is(l) => l,
        LanguageChoice::System => system_locale
            .and_then(|loc| {
                let lang = loc.split(['_', '-', '.', '@']).next().unwrap_or_default();
                Language::parse(lang)
            })
            .unwrap_or(Language::English),
    }
}

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Makes `language` the interface's language and bumps [`generation`].
pub fn set(language: Language) {
    rust_i18n::set_locale(language.code());
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// The current language.
pub fn language() -> Language {
    Language::parse(&rust_i18n::locale()).unwrap_or(Language::English)
}

/// The current language's direction.
pub fn dir() -> Dir {
    language().dir()
}

/// Changes whenever the language does: caches of laid-out or measured text key on it.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

type Interned = HashMap<(Language, String), &'static str>;

/// The text of `key` in the current language.
///
/// The string is interned (leaked once per language and key), so the leak is bounded by
/// keys times languages. A missing key returns the key itself and logs once.
pub fn text(key: &str) -> &'static str {
    text_in(language(), key)
}

/// The text of `key` in `language` (tests, and a language's own name in a menu). See
/// [`text`] for the interning.
pub fn text_in(language: Language, key: &str) -> &'static str {
    static CACHE: OnceLock<Mutex<Interned>> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(s) = cache.get(&(language, key.to_owned())) {
        return s;
    }
    let found = lookup(language, key);
    let leaked: &'static str = match found {
        Some(s) => Box::leak(s.into_boxed_str()),
        None => {
            tracing::warn!(key, language = language.code(), "missing translation key");
            Box::leak(key.to_owned().into_boxed_str())
        }
    };
    cache.insert((language, key.to_owned()), leaked);
    leaked
}

/// The catalogue entry for `key` in `language` (falling back to English), or `None`.
pub(crate) fn lookup(language: Language, key: &str) -> Option<String> {
    let s = t!(key, locale = language.code());
    // rust-i18n answers a missing key with the key (possibly prefixed by the locale).
    if s == key || s.ends_with(key) && s.len() == key.len() + language.code().len() + 1 {
        None
    } else {
        Some(s.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_uses_the_language_part_of_the_locale() {
        let sys = LanguageChoice::System;
        assert_eq!(resolve(sys, Some("ar_EG.UTF-8")), Language::Arabic);
        assert_eq!(resolve(sys, Some("ar")), Language::Arabic);
        assert_eq!(resolve(sys, Some("ar-SA")), Language::Arabic);
        assert_eq!(resolve(sys, Some("en_GB")), Language::English);
        assert_eq!(resolve(sys, Some("fr_FR")), Language::English);
        assert_eq!(resolve(sys, None), Language::English);
        assert_eq!(
            resolve(LanguageChoice::Is(Language::Arabic), Some("en_GB")),
            Language::Arabic
        );
    }

    #[test]
    fn choices_round_trip_and_unknown_means_system() {
        for v in ["system", "en", "ar"] {
            assert_eq!(LanguageChoice::parse(v).value(), v);
        }
        assert_eq!(LanguageChoice::parse("xx"), LanguageChoice::System);
        assert_eq!(
            LanguageChoice::parse("ar-SA"),
            LanguageChoice::Is(Language::Arabic)
        );
    }

    #[test]
    fn names_are_in_the_language_itself() {
        assert_eq!(Language::English.name(), "English");
        assert_eq!(Language::Arabic.name(), "العربية");
        assert_eq!(Language::Arabic.dir(), Dir::Rtl);
        assert_eq!(Language::English.dir(), Dir::Ltr);
    }

    #[test]
    fn arguments_are_filled_in_each_language() {
        assert_eq!(
            t!("sample.greeting", name = "Lantern Keep", locale = "en"),
            "Removed Lantern Keep"
        );
        assert_eq!(
            t!("sample.greeting", name = "Lantern Keep", locale = "ar"),
            "تمت إزالة Lantern Keep"
        );
    }

    #[test]
    fn a_missing_key_comes_back_as_itself() {
        assert_eq!(text_in(Language::English, "no.such.key"), "no.such.key");
    }
}
