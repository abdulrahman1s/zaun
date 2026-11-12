//! Everything specific to `umu-run`: where it is, how it is called, what its output means.
//!
//! Layer 4. See MODULES §5 and UMU_INTEGRATION. Only `locate` exists so far (milestone M3);
//! `command`, `env` and `log_parse` follow in M5.

pub mod locate;
