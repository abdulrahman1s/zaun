//! Screen-reader announcements (ACCESSIBILITY §4.3) and controller hints (USER_EXPERIENCE
//! §5.3).

/// "Hollow Knight: preparing private Windows storage".
pub fn launch_phase(name: &str, phase: &str) -> String {
    let phase = phase.trim_end_matches('…');
    let mut chars = phase.chars();
    let lower = match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect::<String>(),
        None => String::new(),
    };
    format!("{name}: {lower}")
}

/// "Hollow Knight is running".
pub fn running(name: &str) -> String {
    format!("{name} is running")
}

/// "Hollow Knight ended after 1 hour 12 minutes".
pub fn ended_after(name: &str, ms: u64) -> String {
    format!("{name} ended after {}", super::fmt::duration_spoken(ms))
}

/// "Hollow Knight ended".
pub fn ended(name: &str) -> String {
    format!("{name} ended")
}

/// "Downloading, 512 megabytes so far".
pub fn downloading(bytes: u64) -> String {
    let mb = bytes / 1_000_000;
    format!(
        "Downloading, {mb} {} so far",
        super::plural(mb, "megabyte", "megabytes")
    )
}

/// "1 problem: <message>".
pub fn problems(n: usize, first: &str) -> String {
    format!(
        "{n} {}: {first}",
        super::plural(n as u64, "problem", "problems")
    )
}

/// "Added: can read files in Music".
pub fn access_added(statement: &str) -> String {
    format!("Added: {statement}")
}

/// Controller footer (UF-15): button and what it does.
pub const PAD_HINTS: [(&str, &str); 5] = [
    ("A", "Play"),
    ("X", "Details"),
    ("Y", "Favourite"),
    ("B", "Back"),
    ("LB/RB", "Collection"),
];

/// Controller footer on pages and overlays.
pub const PAD_HINTS_PAGE: [(&str, &str); 3] = [("A", "Select"), ("B", "Back"), ("LB/RB", "Tab")];

/// Accessible name of the controller footer.
pub const PAD_HINTS_NAME: &str = "Controller buttons";

/// "Controller buttons. A: Play, X: Details, …".
pub fn pad_hints_spoken(hints: &[(&str, &str)]) -> String {
    let list = hints
        .iter()
        .map(|(b, w)| format!("{b}: {w}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{PAD_HINTS_NAME}. {list}")
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
