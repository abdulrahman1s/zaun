//! Helpers for `insta` snapshots of plans and argument lists with path normalisation.

use std::path::Path;

/// Replace each path (longest first) with its placeholder, e.g. a temporary data root with
/// `<DATA>`, so snapshots are stable across machines.
pub fn normalize_paths(text: &str, replacements: &[(&Path, &str)]) -> String {
    let mut pairs: Vec<(String, &str)> = replacements
        .iter()
        .map(|(p, r)| (p.to_string_lossy().into_owned(), *r))
        .filter(|(p, _)| !p.is_empty())
        .collect();
    pairs.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
    let mut out = text.to_owned();
    for (from, to) in pairs {
        out = out.replace(&from, to);
    }
    out
}

/// Snapshot `text` after [`normalize_paths`]. The snapshot is stored next to the calling test
/// (the macro expands at the call site).
///
/// ```ignore
/// easyplay_testkit::assert_golden!("offline_plan", rendered, &[(env.data(), "<DATA>")]);
/// ```
#[macro_export]
macro_rules! assert_golden {
    ($name:expr, $text:expr, $replacements:expr) => {
        $crate::insta::assert_snapshot!(
            $name,
            $crate::golden::normalize_paths(&$text, $replacements)
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_first() {
        let out = normalize_paths(
            "--bind /tmp/x/data/games /tmp/x/data/games --ro-bind /tmp/x /tmp/x",
            &[
                (Path::new("/tmp/x"), "<ROOT>"),
                (Path::new("/tmp/x/data"), "<DATA>"),
            ],
        );
        assert_eq!(
            out,
            "--bind <DATA>/games <DATA>/games --ro-bind <ROOT> <ROOT>"
        );
    }

    #[test]
    fn snapshot_macro_works() {
        let text = String::from("path=/tmp/abc/file");
        assert_golden!(
            "golden_self_test",
            text,
            &[(Path::new("/tmp/abc"), "<TMP>")]
        );
    }
}
