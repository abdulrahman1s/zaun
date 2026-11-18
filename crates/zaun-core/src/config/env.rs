//! The `ZAUN_*` environment variables (CONFIGURATION §5), parsed in one place.
//!
//! [`EnvOverrides::from_env`] reads a [`PathEnv`] (the process environment captured once at
//! start, or a test environment) and validates every variable. Consumers (paths, logging,
//! host probe, renderer choice) take the resulting struct instead of reading the
//! environment. The directory overrides are also resolved by `paths::AppPaths::resolve`
//! from the same `PathEnv`; they are repeated here so that diagnostics can show them.

use std::path::PathBuf;

use crate::consts::{
    ENV_BWRAP, ENV_CACHE_DIR, ENV_CONFIG_DIR, ENV_DATA_DIR, ENV_INIT, ENV_LOG, ENV_LOG_FORMAT,
    ENV_RENDERER, ENV_STATE_DIR, ENV_UMU_RUN,
};
use crate::paths::PathEnv;

/// `ZAUN_NO_SINGLE_INSTANCE`.
pub const ENV_NO_SINGLE_INSTANCE: &str = "ZAUN_NO_SINGLE_INSTANCE";
/// `ZAUN_E2E`.
pub const ENV_E2E: &str = "ZAUN_E2E";
/// `NO_COLOR`.
pub const ENV_NO_COLOR: &str = "NO_COLOR";

/// Renderer forced for this run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RendererOverride {
    /// `wgpu`.
    Wgpu,
    /// `glow`.
    Glow,
}

/// Log output format for this run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LogFormat {
    /// Human-readable text.
    Text,
    /// JSON lines.
    Json,
}

/// A variable that was set but ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvWarning {
    /// Variable name.
    pub var: &'static str,
    /// Why it was ignored.
    pub reason: String,
}

/// Every environment input of CONFIGURATION §5 except the XDG and display variables.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnvOverrides {
    /// `ZAUN_CONFIG_DIR`.
    pub config_dir: Option<PathBuf>,
    /// `ZAUN_DATA_DIR`.
    pub data_dir: Option<PathBuf>,
    /// `ZAUN_CACHE_DIR`.
    pub cache_dir: Option<PathBuf>,
    /// `ZAUN_STATE_DIR`.
    pub state_dir: Option<PathBuf>,
    /// `ZAUN_LOG`: a level or `target=level` pairs, validated.
    pub log: Option<String>,
    /// `ZAUN_LOG_FORMAT`.
    pub log_format: Option<LogFormat>,
    /// `ZAUN_RENDERER`.
    pub renderer: Option<RendererOverride>,
    /// `ZAUN_INIT`.
    pub init: Option<PathBuf>,
    /// `ZAUN_UMU_RUN`.
    pub umu_run: Option<PathBuf>,
    /// `ZAUN_BWRAP`.
    pub bwrap: Option<PathBuf>,
    /// `ZAUN_NO_SINGLE_INSTANCE` (development; the caller refuses it with default roots).
    pub no_single_instance: bool,
    /// `ZAUN_E2E`.
    pub e2e: bool,
    /// `NO_COLOR` (any non-empty value).
    pub no_color: bool,
}

const LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace", "off"];

/// `level` or comma-separated `target=level` / `level` items.
fn valid_log_spec(spec: &str) -> bool {
    !spec.is_empty()
        && spec.split(',').all(|item| {
            let item = item.trim();
            let level = match item.split_once('=') {
                Some((target, level)) => {
                    let target_ok = !target.is_empty()
                        && target
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'-'));
                    if !target_ok {
                        return false;
                    }
                    level
                }
                None => item,
            };
            LEVELS.iter().any(|l| l.eq_ignore_ascii_case(level))
        })
}

fn truthy(v: &str) -> bool {
    !matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "no" | "off"
    )
}

struct Parser<'a> {
    env: &'a PathEnv,
    warnings: Vec<EnvWarning>,
}

impl Parser<'_> {
    fn warn(&mut self, var: &'static str, reason: &str) {
        self.warnings.push(EnvWarning {
            var,
            reason: reason.into(),
        });
    }

    /// A set, non-empty, UTF-8 value.
    fn text(&mut self, var: &'static str) -> Option<String> {
        let v = self.env.get(var)?;
        match v.to_str() {
            Some("") => None,
            Some(s) => Some(s.to_owned()),
            None => {
                self.warn(var, "not valid UTF-8");
                None
            }
        }
    }

    /// An absolute path (non-UTF-8 bytes allowed).
    fn abs(&mut self, var: &'static str) -> Option<PathBuf> {
        let v = self.env.get(var)?;
        if v.is_empty() {
            return None;
        }
        let p = PathBuf::from(v);
        if p.is_absolute() {
            Some(crate::paths::normalize(&p))
        } else {
            self.warn(var, "must be an absolute path");
            None
        }
    }

    fn choice<T: Copy>(
        &mut self,
        var: &'static str,
        table: &[(&str, T)],
        expected: &str,
    ) -> Option<T> {
        let v = self.text(var)?;
        let found = table
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(v.trim()))
            .map(|(_, t)| *t);
        if found.is_none() {
            self.warn(var, expected);
        }
        found
    }

    fn flag(&mut self, var: &'static str) -> bool {
        self.text(var).is_some_and(|v| truthy(&v))
    }
}

impl EnvOverrides {
    /// Parse and validate. Invalid values are ignored and reported.
    pub fn from_env(env: &PathEnv) -> (EnvOverrides, Vec<EnvWarning>) {
        let mut p = Parser {
            env,
            warnings: Vec::new(),
        };
        let log = p.text(ENV_LOG).and_then(|v| {
            if valid_log_spec(&v) {
                Some(v)
            } else {
                p.warn(
                    ENV_LOG,
                    "expected a level (error … trace) or target=level pairs",
                );
                None
            }
        });
        let out = EnvOverrides {
            config_dir: p.abs(ENV_CONFIG_DIR),
            data_dir: p.abs(ENV_DATA_DIR),
            cache_dir: p.abs(ENV_CACHE_DIR),
            state_dir: p.abs(ENV_STATE_DIR),
            log,
            log_format: p.choice(
                ENV_LOG_FORMAT,
                &[("json", LogFormat::Json), ("text", LogFormat::Text)],
                "expected json or text",
            ),
            renderer: p.choice(
                ENV_RENDERER,
                &[
                    ("wgpu", RendererOverride::Wgpu),
                    ("glow", RendererOverride::Glow),
                ],
                "expected wgpu or glow",
            ),
            init: p.abs(ENV_INIT),
            umu_run: p.abs(ENV_UMU_RUN),
            bwrap: p.abs(ENV_BWRAP),
            no_single_instance: p.flag(ENV_NO_SINGLE_INSTANCE),
            e2e: p.flag(ENV_E2E),
            no_color: p.text(ENV_NO_COLOR).is_some(),
        };
        (out, p.warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(vars: &[(&str, &str)]) -> (EnvOverrides, Vec<EnvWarning>) {
        let mut env = PathEnv::new(1000);
        for (k, v) in vars {
            env.set(*k, *v);
        }
        EnvOverrides::from_env(&env)
    }

    #[test]
    fn empty_environment_overrides_nothing() {
        let (o, w) = parse(&[]);
        assert_eq!(o, EnvOverrides::default());
        assert!(w.is_empty());
    }

    #[test]
    fn table() {
        let cases: &[(&str, &str, bool)] = &[
            (ENV_LOG, "debug", true),
            (ENV_LOG, "info,zaun_core::storage=trace", true),
            (ENV_LOG, "loud", false),
            (ENV_LOG, "a b=info", false),
            (ENV_RENDERER, "GLOW", true),
            (ENV_RENDERER, "vulkan", false),
            (ENV_LOG_FORMAT, "json", true),
            (ENV_LOG_FORMAT, "xml", false),
            (ENV_DATA_DIR, "/srv/zaun", true),
            (ENV_DATA_DIR, "relative/dir", false),
            (ENV_BWRAP, "/opt/bwrap", true),
            (ENV_UMU_RUN, "umu-run", false),
            (ENV_INIT, "/usr/libexec/zaun-init", true),
            (ENV_E2E, "1", true),
        ];
        for (var, value, ok) in cases {
            let (_, w) = parse(&[(var, value)]);
            assert_eq!(w.is_empty(), *ok, "{var}={value}: {w:?}");
        }
        let (o, _) = parse(&[
            (ENV_RENDERER, "glow"),
            (ENV_DATA_DIR, "/srv/zaun/../ep"),
            (ENV_E2E, "1"),
            (ENV_NO_SINGLE_INSTANCE, "0"),
            (ENV_NO_COLOR, "yes"),
            (ENV_LOG_FORMAT, "JSON"),
        ]);
        assert_eq!(o.renderer, Some(RendererOverride::Glow));
        assert_eq!(o.data_dir, Some(PathBuf::from("/srv/ep")));
        assert!(o.e2e);
        assert!(!o.no_single_instance);
        assert!(o.no_color);
        assert_eq!(o.log_format, Some(LogFormat::Json));
    }
}
