//! Small macros for shapes the crate repeats: building and returning errors (ADR-0034).

/// An [`Error`](crate::Error) with a code and context.
///
/// `err!(LibGameNotFound, game = id)` is
/// `Error::new(ErrorCode::LibGameNotFound).with("game", id)`.
#[macro_export]
macro_rules! err {
    ($code:ident $(, $key:ident = $value:expr)* $(,)?) => {
        $crate::error::Error::new($crate::error::ErrorCode::$code)
            $(.with(::core::stringify!($key), $value))*
    };
}

/// Return `Err(err!(…))` from the enclosing function: `bail!(SesAlreadyRunning, game = id)`.
#[macro_export]
macro_rules! bail {
    ($($err:tt)+) => {
        return ::core::result::Result::Err($crate::err!($($err)+).into())
    };
}

/// `bail!` unless the condition holds: `ensure!(!busy, SesAlreadyRunning, game = id)`.
#[macro_export]
macro_rules! ensure {
    ($cond:expr, $($err:tt)+) => {
        if !$cond {
            $crate::bail!($($err)+);
        }
    };
}

/// A fieldless enum that is written as fixed words (`config.toml`, wire formats).
///
/// Generates the enum (serde by the word), `as_str`, `ALL` in declared order, `FromStr`
/// (case-insensitive, `Err = ()`), `Display` and, with `default`, `Default`.
///
/// ```ignore
/// str_enum! {
///     /// `appearance.theme`.
///     pub Theme { System = "system", HighContrast = "high-contrast" } default System
/// }
/// assert_eq!(Theme::HighContrast.as_str(), "high-contrast");
/// ```
#[macro_export]
macro_rules! str_enum {
    ($(#[$m:meta])* $vis:vis $name:ident { $($(#[$vm:meta])* $v:ident = $s:literal),+ $(,)? } $(default $d:ident)?) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ::serde::Serialize, ::serde::Deserialize)]
        $vis enum $name {
            $( $(#[$vm])* #[serde(rename = $s)] $v, )+
        }
        impl $name {
            /// Every value, in declared order.
            pub const ALL: &'static [$name] = &[$($name::$v),+];
            /// The word the value is written as.
            pub const fn as_str(self) -> &'static str {
                match self { $($name::$v => $s),+ }
            }
        }
        $(impl ::core::default::Default for $name {
            fn default() -> Self { $name::$d }
        })?
        impl ::core::str::FromStr for $name {
            type Err = ();
            fn from_str(s: &str) -> ::core::result::Result<Self, ()> {
                Self::ALL.iter().copied().find(|v| v.as_str().eq_ignore_ascii_case(s)).ok_or(())
            }
        }
        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::{Error, ErrorCode, Result};

    crate::str_enum! {
        Mode { Fast = "fast", Slow = "slow" } default Fast
    }

    #[test]
    fn str_enum_round_trips_its_words() {
        assert_eq!(Mode::default(), Mode::Fast);
        assert_eq!(Mode::ALL, &[Mode::Fast, Mode::Slow]);
        assert_eq!("SLOW".parse(), Ok(Mode::Slow));
        assert_eq!(Mode::Slow.to_string(), "slow");
        assert!("x".parse::<Mode>().is_err());
    }

    fn check(ok: bool) -> Result<u8> {
        crate::ensure!(ok, LibGameNotFound, game = 7, why = "gone");
        Ok(1)
    }

    #[test]
    fn err_builds_code_and_context_in_order() {
        let e: Error = crate::err!(LibInvalidField, field = "name", detail = 3);
        assert_eq!(e.code, ErrorCode::LibInvalidField);
        assert_eq!(e.context_value("field"), Some("name"));
        assert_eq!(e.context_value("detail"), Some("3"));
        assert_eq!(crate::err!(IntTaskFailed).context.len(), 0);
    }

    #[test]
    fn ensure_returns_the_error_only_when_the_condition_fails() {
        assert_eq!(check(true).ok(), Some(1));
        let e = check(false).err().map(|e| (e.code, e.context));
        assert_eq!(
            e,
            Some((
                ErrorCode::LibGameNotFound,
                vec![("game", "7".to_owned()), ("why", "gone".to_owned())]
            ))
        );
    }
}
