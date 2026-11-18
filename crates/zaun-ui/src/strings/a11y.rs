//! Screen-reader announcements (ACCESSIBILITY §4.3) and controller hints (USER_EXPERIENCE
//! §5.3).

/// "Hollow Knight: preparing private Windows storage".
pub fn launch_phase(name: &str, phase: &str) -> String {
    let phase = phase.trim_end_matches('…');
    let mut chars = phase.chars();
    // Lower-casing the first letter is a no-op in scripts without case.
    let lower = match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect::<String>(),
        None => String::new(),
    };
    rust_i18n::t!("a11y.launch_phase", name = name, phase = lower).into_owned()
}

/// "Hollow Knight is running".
pub fn running(name: &str) -> String {
    rust_i18n::t!("a11y.running", name = name).into_owned()
}

/// "Hollow Knight ended after 1 hour 12 minutes".
pub fn ended_after(name: &str, ms: u64) -> String {
    let after = super::fmt::duration_spoken(ms);
    rust_i18n::t!("a11y.ended_after", name = name, after = after).into_owned()
}

/// "Hollow Knight ended".
pub fn ended(name: &str) -> String {
    rust_i18n::t!("a11y.ended", name = name).into_owned()
}

/// "Downloading, 512 megabytes so far".
pub fn downloading(bytes: u64) -> String {
    crate::i18n::plural::counted("a11y.downloading", bytes / 1_000_000)
}

/// "1 problem: <message>".
pub fn problems(n: usize, first: &str) -> String {
    crate::i18n::plural::counted_args("a11y.problems", n as u64, &[("first", first)])
}

/// "Added: can read files in Music".
pub fn access_added(statement: &str) -> String {
    rust_i18n::t!("a11y.access_added", statement = statement).into_owned()
}

/// Controller footer (UF-15): button and what it does.
#[allow(non_snake_case)]
pub fn PAD_HINTS() -> [(&'static str, &'static str); 5] {
    [
        ("A", crate::i18n::text("a11y.pad_hints.play")),
        ("X", crate::i18n::text("a11y.pad_hints.details")),
        ("Y", crate::i18n::text("a11y.pad_hints.favourite")),
        ("B", crate::i18n::text("a11y.pad_hints.back")),
        ("LB/RB", crate::i18n::text("a11y.pad_hints.collection")),
    ]
}

/// Controller footer on pages and overlays.
#[allow(non_snake_case)]
pub fn PAD_HINTS_PAGE() -> [(&'static str, &'static str); 3] {
    [
        ("A", crate::i18n::text("a11y.pad_hints_page.select")),
        ("B", crate::i18n::text("a11y.pad_hints_page.back")),
        ("LB/RB", crate::i18n::text("a11y.pad_hints_page.tab")),
    ]
}

texts! {
    /// Accessible name of the controller footer.
    PAD_HINTS_NAME = "a11y.pad_hints_name";
}

/// "Controller buttons. A: Play, X: Details, …".
pub fn pad_hints_spoken(hints: &[(&str, &str)]) -> String {
    let list = hints
        .iter()
        .map(|(b, w)| rust_i18n::t!("a11y.pad_hint", button = b, what = w).into_owned())
        .collect::<Vec<_>>()
        .join(crate::i18n::text("list.separator"));
    rust_i18n::t!(
        "a11y.pad_hints_spoken",
        name = PAD_HINTS_NAME(),
        list = list
    )
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcements_read_naturally() {
        assert_eq!(
            launch_phase("Hollow Knight", "Preparing private Windows storage…"),
            "Hollow Knight: preparing private Windows storage"
        );
        assert_eq!(
            ended_after("Hollow Knight", 72 * 60_000),
            "Hollow Knight ended after 1 hour 12 minutes"
        );
        assert_eq!(
            downloading(512_400_000),
            "Downloading, 512 megabytes so far"
        );
        assert_eq!(problems(1, "Name is empty"), "1 problem: Name is empty");
    }
}
