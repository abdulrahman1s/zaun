//! Everything specific to `umu-run`: where it is, how it is called, what its output means.
//!
//! Layer 4. See MODULES §5 and UMU_INTEGRATION. [`locate`] (M3) finds the program and its
//! closure; [`env`] holds the variable tables and user-variable rules; [`flags`] the Proton
//! options the Launch options tab suggests; [`command`] builds the invocation per session
//! kind; [`log_parse`] classifies output, maps failures to error codes and interprets exit
//! statuses.

pub mod command;
pub mod env;
pub mod flags;
pub mod locate;
pub mod log_parse;

pub use command::{
    CommandError, PROVISION_PREFIX, ProtonPath, Target, UmuGame, UmuInvocation, UmuOptions, build,
};
pub use env::{EnvRule, check_user_var, classify, compat_vars, is_never_set};
pub use log_parse::{
    ExitMeaning, LineClass, LogClassifier, NoiseKind, UmuErrorKind, UmuFailure, UmuPhase,
    interpret_exit, map_message, sanitize_line,
};
