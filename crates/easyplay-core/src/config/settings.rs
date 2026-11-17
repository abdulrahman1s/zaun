//! `settings!`: one table per `[section]` of `config.toml`, so a setting is written once.
//!
//! ```ignore
//! settings! {
//!     /// `[general]`.
//!     General "general" {
//!         /// `"system"` or a BCP-47 tag.
//!         language: String = "system".into() => custom(language),
//!         close_behavior: CloseBehavior = CloseBehavior::Quit => word,
//!         ui_scale: f64 = 1.0 => float(0.75, 2.0),
//!     }
//! }
//! ```
//!
//! Each section becomes a struct (fields in this order: that is the order of the written
//! file), its `Default` (the `=` values), and `read`, which reads every key from a
//! [`Reader`](super::reader::Reader): a wrong type or value warns and gives the default, a
//! number outside `lo..=hi` is clamped. Reader kinds: `bool`, `word` (a `str_enum!`
//! type), `string`, `int(lo, hi)` (any integer type), `float(lo, hi)`, and
//! `custom(f)` for the irregular, where `f` is a `config::custom` function.

macro_rules! settings {
    (@read $r:ident $s:ident $key:expr, $ty:ty, $d:expr, bool) => {
        $r.bool(&mut $s, $key, $d)
    };
    (@read $r:ident $s:ident $key:expr, $ty:ty, $d:expr, word) => {
        $r.word(
            &mut $s, $key, $d,
            &$crate::config::reader::names(<$ty>::ALL, <$ty>::as_str),
        )
    };
    (@read $r:ident $s:ident $key:expr, $ty:ty, $d:expr, string) => {
        $r.string(&mut $s, $key).unwrap_or($d)
    };
    (@read $r:ident $s:ident $key:expr, $ty:ty, $d:expr, int($lo:expr, $hi:expr)) => {
        <$ty>::try_from($r.int(&mut $s, $key, i64::from($d), $lo, $hi)).unwrap_or($d)
    };
    (@read $r:ident $s:ident $key:expr, $ty:ty, $d:expr, float($lo:expr, $hi:expr)) => {
        $r.float(&mut $s, $key, $d, $lo, $hi)
    };
    (@read $r:ident $s:ident $key:expr, $ty:ty, $d:expr, custom($f:path)) => {
        $f($r, &mut $s, $key, $d)
    };
    ($(
        $(#[$m:meta])*
        $name:ident $sec:literal {
            $($(#[$fm:meta])* $f:ident : $ty:ty = $d:expr => $kind:ident $(($($arg:tt)*))?),+ $(,)?
        }
    )+) => {$(
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq, Serialize)]
        pub struct $name {
            $($(#[$fm])* pub $f: $ty,)+
        }

        impl Default for $name {
            fn default() -> Self {
                Self { $($f: $d),+ }
            }
        }

        impl $name {
            /// Take `[section]` out of `root` and read its keys.
            pub(super) fn read(r: &mut $crate::config::reader::Reader<'_>, root: &mut toml::Table) -> Self {
                let mut s = r.section(root, $sec);
                let out = Self {
                    $($f: settings!(@read r s stringify!($f), $ty, $d, $kind $(($($arg)*))?)),+
                };
                r.finish(s);
                out
            }
        }
    )+};
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    use crate::config::load::{ConfigProblem, LoadOptions};
    use crate::config::reader::Reader;
    use crate::fsx::RealFs;

    settings! {
        /// `[probe]`.
        Probe "probe" {
            on: bool = true => bool,
            scale: f64 = 1.0 => float(0.5, 2.0),
            hours: u32 = 24 => int(6, 720),
        }
    }

    #[test]
    fn wrong_type_and_out_of_range_warn_and_fall_back() {
        let mut root: toml::Table = "[probe]\non = \"yes\"\nscale = 9.0\nhours = 1\nextra = 1"
            .parse()
            .unwrap_or_default();
        let (fs, opts) = (RealFs::new(), LoadOptions::default());
        let mut r = Reader::new(&fs, &opts);
        let p = Probe::read(&mut r, &mut root);
        assert_eq!(
            p,
            Probe {
                on: true,
                scale: 2.0,
                hours: 6
            }
        );
        let seen: Vec<_> = r
            .warnings
            .iter()
            .map(|w| (w.key.as_str(), &w.problem))
            .collect();
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].0, "probe.on");
        assert!(matches!(seen[0].1, ConfigProblem::InvalidValue { .. }));
        assert_eq!(
            seen[1],
            ("probe.scale", &ConfigProblem::Clamped { to: "2".into() })
        );
        assert_eq!(
            seen[2],
            ("probe.hours", &ConfigProblem::Clamped { to: "6".into() })
        );
        assert!(r.unknown.contains_key("probe"));
    }

    #[test]
    fn a_wrong_type_for_a_number_gives_the_default() {
        let mut root: toml::Table = "[probe]\nscale = \"big\"\nhours = true"
            .parse()
            .unwrap_or_default();
        let (fs, opts) = (RealFs::new(), LoadOptions::default());
        let mut r = Reader::new(&fs, &opts);
        assert_eq!(Probe::read(&mut r, &mut root), Probe::default());
        assert_eq!(r.warnings.len(), 2);
    }
}
