//! Mentions in a message's text: `@name` and `@'two words'` (Markdown.hs `mentionP`, `displayNameTextP_`, `viewName`).
//!
//! Only the mention rule is ported, not the whole markdown parser: an `@` starts a mention at
//! the start of the text or after whitespace, as markdown fragments do. Code spans and
//! SimpleX names (`@name.with.dots` that parse as one) are not told apart.

/// Markdown.hs `refChar`: what may open a name.
fn ref_char(c: char) -> bool {
    c > ' ' && c != '#' && c != '@' && c != '\''
}

/// One mention at the start of `s` (just after the `@`): the name, and the bytes it took.
fn mention_at(s: &str) -> Option<(&str, usize)> {
    if let Some(rest) = s.strip_prefix('\'') {
        let end = rest.find(['\'', '\n'])?;
        let name = &rest[..end];
        let ok = name.starts_with(ref_char) && rest[end..].starts_with('\'');
        return ok.then_some((name, end + 2));
    }
    if !s.starts_with(ref_char) {
        return None;
    }
    let word = &s[..s.find(char::is_whitespace).unwrap_or(s.len())];
    // `splitPunctuation`: trailing punctuation is not part of the name.
    let name = word.trim_end_matches(|c: char| c.is_ascii_punctuation());
    (!name.is_empty()).then_some((name, word.len()))
}

/// Markdown.hs `mentionedNames`, in the order they appear.
#[must_use]
pub(crate) fn mentioned_names(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut at_start = true;
    let mut i = 0;
    while let Some(c) = text[i..].chars().next() {
        if c == '@'
            && at_start
            && let Some((name, len)) = mention_at(&text[i + 1..])
        {
            out.push(name);
            i += 1 + len;
            at_start = false;
            continue;
        }
        at_start = c.is_whitespace();
        i += c.len_utf8();
    }
    out
}

/// Markdown.hs `viewName`: a name with a space or ending on punctuation is quoted.
#[must_use]
pub(crate) fn view_name(name: &str) -> String {
    let quote =
        name.contains(char::is_whitespace) || name.ends_with(|c: char| c.is_ascii_punctuation());
    if quote {
        format!("'{name}'")
    } else {
        name.to_owned()
    }
}

/// `@Two Words` in the page's text becomes `@'Two Words'`, the form the apps parse (codec `quote_mentions`).
#[must_use]
pub(crate) fn quote_mentions<'a>(text: &str, names: impl Iterator<Item = &'a str>) -> String {
    let mut text = text.to_owned();
    for name in names {
        let shown = view_name(name);
        if shown != name {
            text = text.replace(&format!("@{name}"), &format!("@{shown}"));
        }
    }
    text
}

/// `@'Two Words'` back to `@Two Words` for the names a message mentions (codec `unquote_mentions`).
#[must_use]
pub(crate) fn unquote_mentions<'a>(text: &str, names: impl Iterator<Item = &'a str>) -> String {
    let mut text = text.to_owned();
    for name in names {
        text = text.replace(&format!("@'{name}'"), &format!("@{name}"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mention_starts_a_word_and_drops_trailing_punctuation() {
        assert_eq!(
            mentioned_names("@bob ready? hi @carol, mail a@b.c"),
            ["bob", "carol"]
        );
    }

    #[test]
    fn a_quoted_mention_holds_spaces() {
        assert_eq!(mentioned_names("@'Bob Smith' quoted @'open"), ["Bob Smith"]);
        assert_eq!(view_name("Bob Smith"), "'Bob Smith'");
        assert_eq!(view_name("bob"), "bob");
    }

    #[test]
    fn quoting_and_unquoting_mentions_round_trip() {
        let sent = quote_mentions("@Bob Smith quoted", ["Bob Smith"].into_iter());
        assert_eq!(sent, "@'Bob Smith' quoted");
        assert_eq!(
            unquote_mentions(&sent, ["Bob Smith"].into_iter()),
            "@Bob Smith quoted"
        );
    }
}
