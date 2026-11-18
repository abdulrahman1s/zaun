//! Global preferences in `config.toml` and environment overrides.
//!
//! Layer 3. See MODULES §4 and CONFIGURATION.
//!
//! - [`schema`]: [`Preferences`] with every key of CONFIGURATION §2 and its default,
//!   [`PreferencePatch`] for partial updates.
//! - [`load`](mod@load): [`load()`](load::load) validates and clamps key by key, producing
//!   [`ConfigWarning`]s; unknown keys are kept; [`save`] writes atomically.
//! - [`migrate`]: version-to-version upgrades of the TOML document.
//! - [`env`]: the `ZAUN_*` variables, parsed once into [`EnvOverrides`].
//!
//! `config` stores no per-game data, secrets or UI layout state (that is `KvRepo`).

#[macro_use]
mod settings;

mod custom;
pub mod env;
pub mod load;
pub mod migrate;
mod reader;
pub mod schema;

pub use env::{EnvOverrides, EnvWarning, LogFormat, RendererOverride};
pub use load::{
    ConfigError, ConfigProblem, ConfigSource, ConfigWarning, EnvRule, LoadOptions,
    LoadedPreferences, save, to_toml_string,
};
pub use schema::{ColumnWidth, PreferencePatch, Preferences, RuntimeDefault, UnknownKeys};
