//! Plural categories (CLDR cardinal rules) for the shipped languages; rust-i18n has none.

use super::{Language, lookup};

/// A CLDR plural category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    /// Arabic 0.
    Zero,
    /// 1.
    One,
    /// Arabic 2.
    Two,
    /// Arabic 3-10 (mod 100).
    Few,
    /// Arabic 11-99 (mod 100).
    Many,
    /// Everything else.
    Other,
}

impl Category {
    /// The catalogue key segment: "zero", "one", ...
    pub const fn name(self) -> &'static str {
        match self {
            Category::Zero => "zero",
            Category::One => "one",
            Category::Two => "two",
            Category::Few => "few",
            Category::Many => "many",
            Category::Other => "other",
        }
    }
}

/// The plural category of `n` in `language`.
pub fn category(language: Language, n: u64) -> Category {
    match language {
        Language::English => {
            if n == 1 {
                Category::One
            } else {
                Category::Other
            }
        }
        Language::Arabic => match n {
            0 => Category::Zero,
            1 => Category::One,
            2 => Category::Two,
            _ => match n % 100 {
                3..=10 => Category::Few,
                11..=99 => Category::Many,
                _ => Category::Other,
            },
        },
    }
}

/// The plural family `key` (children `zero`..`other`) for `n` in `language`, with `%{n}`
/// filled in. Falls back to `key.other`, then to the key itself. Digits are always 0-9.
pub fn counted_in(language: Language, key: &str, n: u64) -> String {
    let cat = category(language, n);
    let text = lookup(language, &format!("{key}.{}", cat.name()))
        .or_else(|| lookup(language, &format!("{key}.other")))
        .unwrap_or_else(|| key.to_owned());
    text.replace("%{n}", &n.to_string())
}

/// [`counted_in`] with more named arguments: each `%{name}` is replaced by its value.
pub fn counted_args_in(language: Language, key: &str, n: u64, args: &[(&str, &str)]) -> String {
    let mut text = counted_in(language, key, n);
    for (name, value) in args {
        text = text.replace(&format!("%{{{name}}}"), value);
    }
    text
}

/// [`counted_args_in`] for the current language.
pub fn counted_args(key: &str, n: u64, args: &[(&str, &str)]) -> String {
    counted_args_in(super::language(), key, n, args)
}

/// [`counted_in`] for the current language.
pub fn counted(key: &str, n: u64) -> String {
    counted_in(super::language(), key, n)
}

#[cfg(test)]
mod tests {
    use super::Category::*;
    use super::*;

    #[test]
    fn arabic_categories() {
        let cases = [
            (0, Zero),
            (1, One),
            (2, Two),
            (3, Few),
            (10, Few),
            (11, Many),
            (99, Many),
            (100, Other),
            (101, Other),
            (102, Other),
            (103, Few),
            (111, Many),
            (1000, Other),
        ];
        for (n, want) in cases {
            assert_eq!(category(Language::Arabic, n), want, "{n}");
        }
    }

    #[test]
    fn english_categories() {
        assert_eq!(category(Language::English, 0), Other);
        assert_eq!(category(Language::English, 1), One);
        assert_eq!(category(Language::English, 2), Other);
    }

    #[test]
    fn counted_fills_the_number_in_both_languages() {
        let en = |n| counted_in(Language::English, "sample.games", n);
        assert_eq!(en(0), "0 games");
        assert_eq!(en(1), "1 game");
        assert_eq!(en(2), "2 games");
        let ar = |n| counted_in(Language::Arabic, "sample.games", n);
        assert_eq!(ar(0), "لا ألعاب");
        assert_eq!(ar(1), "لعبة واحدة");
        assert_eq!(ar(2), "لعبتان");
        assert_eq!(ar(3), "3 ألعاب");
        assert_eq!(ar(11), "11 لعبة");
        assert_eq!(ar(100), "100 لعبة");
    }

    #[test]
    fn a_missing_family_comes_back_as_its_key() {
        assert_eq!(
            counted_in(Language::English, "nope.family", 3),
            "nope.family"
        );
    }
}
