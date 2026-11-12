//! Classification of user-defined variables (UMU_INTEGRATION §4 table D) and the never-set
//! list.
//!
//! The rules live in [`crate::umu::env`], their single source; this module only gives them
//! the names the planner and the validator use. `umu::env` is pure data and functions over
//! `model` types, so the dependency does not let `sandbox` know anything else about umu.

pub use crate::umu::env::{EnvRule as UserVarRule, classify as classify_user_var, is_never_set};
