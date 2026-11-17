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

#[cfg(test)]
mod tests {
    use crate::{Error, ErrorCode, Result};

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
