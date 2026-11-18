//! Common error envelope, stable error codes, mapping to user-facing errors.
//!
//! Layer 1. Each core module has its own `thiserror` enum convertible into [`Error`], which
//! carries a stable [`ErrorCode`], key/value context and the source chain. Only the service
//! layer converts an `Error` into a [`UserError`], through [`catalog::describe`], which is the
//! only place that writes titles, messages and action lists (ERROR_HANDLING §2).
//!
//! Dependencies: `ids`, and `model::BasePreset` for [`RecoveryAction::SwitchPreset`]. `model`
//! in turn uses only [`ErrorCode`] (from `code.rs`), so there is no cycle between files.

pub mod catalog;
mod code;
mod user;

use std::fmt;

pub use code::{ErrorCode, UnknownErrorCode};
pub use user::{RecoveryAction, SettingsTab, Severity, UserError};

/// A boxed error source.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// The common error envelope.
pub struct Error {
    /// Stable code.
    pub code: ErrorCode,
    /// Key → value context: `path`, `game`, `variant`, `errno`, `task`…
    pub context: Vec<(&'static str, String)>,
    /// The underlying cause.
    pub source: Option<BoxError>,
}

/// Result with the common envelope as the default error.
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    /// An error with a code and no context.
    pub fn new(code: ErrorCode) -> Self {
        Self {
            code,
            context: Vec::new(),
            source: None,
        }
    }

    /// Add one context entry.
    #[must_use]
    pub fn with(mut self, key: &'static str, value: impl fmt::Display) -> Self {
        self.context.push((key, value.to_string()));
        self
    }

    /// Attach the underlying cause.
    #[must_use]
    pub fn with_source(mut self, source: impl Into<BoxError>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// The placeholder for functionality of a later milestone: `ZN-INT-003` with the
    /// tracking task ID in the context (IMPLEMENTATION_ORDER §2).
    pub fn not_available(task: &'static str) -> Self {
        Self::new(ErrorCode::IntNotAvailable).with("task", task)
    }

    /// The first context value stored under `key`.
    pub fn context_value(&self, key: &str) -> Option<&str> {
        self.context
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The source chain rendered as `outer: inner: …` (unredacted; pass through
    /// `logging::redact` before showing it).
    pub fn chain_text(&self) -> String {
        let mut out = String::new();
        let mut next: Option<&(dyn std::error::Error + 'static)> = self
            .source
            .as_deref()
            .map(|e| e as &(dyn std::error::Error + 'static));
        while let Some(e) = next {
            if !out.is_empty() {
                out.push_str(": ");
            }
            out.push_str(&e.to_string());
            next = e.source();
        }
        out
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code.as_str())?;
        for (i, (k, v)) in self.context.iter().enumerate() {
            f.write_str(if i == 0 { " (" } else { ", " })?;
            write!(f, "{k}={v}")?;
        }
        if !self.context.is_empty() {
            f.write_str(")")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("code", &self.code)
            .field("context", &self.context)
            .field("source", &self.source.as_ref().map(|s| s.to_string()))
            .finish()
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

impl From<ErrorCode> for Error {
    fn from(code: ErrorCode) -> Self {
        Self::new(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, thiserror::Error)]
    #[error("outer")]
    struct Outer(#[source] std::io::Error);

    #[test]
    fn display_includes_code_and_context() {
        let e = Error::new(ErrorCode::LibExeNotFound)
            .with("path", "~/Games/x.exe")
            .with("game", "0192f0c2");
        assert_eq!(
            e.to_string(),
            "ZN-LIB-001 (path=~/Games/x.exe, game=0192f0c2)"
        );
        assert_eq!(e.context_value("game"), Some("0192f0c2"));
        assert_eq!(Error::new(ErrorCode::DbBusy).to_string(), "ZN-DB-002");
    }

    #[test]
    fn source_chain() {
        let e = Error::new(ErrorCode::FsWriteDenied)
            .with_source(Outer(std::io::Error::other("inner cause")));
        assert_eq!(e.chain_text(), "outer: inner cause");
        assert!(std::error::Error::source(&e).is_some());
    }

    #[test]
    fn not_available_carries_task() {
        let e = Error::not_available("CLI-001");
        assert_eq!(e.code, ErrorCode::IntNotAvailable);
        assert_eq!(e.context_value("task"), Some("CLI-001"));
    }
}
