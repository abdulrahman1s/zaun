//! Readers of the settings that are not a plain bool, word, number or string (`custom(fn)` in `settings!`).

use std::path::{Path, PathBuf};

use toml::Value;

use super::load::{EnvRule, MAX_EXTRA_SEARCH_DIRS, MAX_GLOBAL_ENV, SEARCH_DIR_DENY};
use super::reader::{Reader, Section};
use super::schema::{ColumnWidth, RuntimeDefault};
use crate::model::{BasePreset, EnvVar};

pub(super) fn language(r: &mut Reader<'_>, s: &mut Section, key: &str, default: String) -> String {
    match r.string(s, key) {
        None => default,
        Some(l) if valid_language(&l) => l,
        Some(l) => {
            r.invalid(s, key, format!("{l:?} is not \"system\" or a language tag"));
            default
        }
    }
}

fn valid_language(s: &str) -> bool {
    if s == "system" {
        return true;
    }
    let mut parts = s.split('-');
    let first_ok = parts
        .next()
        .is_some_and(|p| (2..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphabetic()));
    first_ok
        && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

pub(super) fn base_preset(
    r: &mut Reader<'_>,
    s: &mut Section,
    key: &str,
    default: BasePreset,
) -> BasePreset {
    let Some(p) = r.string(s, key) else {
        return default;
    };
    BasePreset::parse(p.trim()).unwrap_or_else(|| {
        r.invalid(
            s,
            key,
            format!("{p:?} is not one of offline, online, standard"),
        );
        default
    })
}

pub(super) fn runtime_default(
    r: &mut Reader<'_>,
    s: &mut Section,
    key: &str,
    default: RuntimeDefault,
) -> RuntimeDefault {
    let Some(v) = r.string(s, key) else {
        return default;
    };
    v.trim().parse().unwrap_or_else(|e: String| {
        r.invalid(s, key, e);
        default
    })
}

pub(super) fn column_widths(
    r: &mut Reader<'_>,
    s: &mut Section,
    key: &str,
    _default: Vec<ColumnWidth>,
) -> Vec<ColumnWidth> {
    let mut out: Vec<ColumnWidth> = Vec::new();
    for (i, v) in r.list(s, key).into_iter().enumerate() {
        let Ok(w) = v.try_into::<ColumnWidth>() else {
            r.dropped(s, key, i, "expected { column = \"…\", width = … }");
            continue;
        };
        if !ColumnWidth::COLUMNS.contains(&w.column.as_str()) {
            r.dropped(s, key, i, format!("unknown column {:?}", w.column));
        } else if out.iter().any(|x| x.column == w.column) {
            r.dropped(s, key, i, "listed twice");
        } else {
            out.push(ColumnWidth {
                width: w.width.clamp(ColumnWidth::MIN, ColumnWidth::MAX),
                column: w.column,
            });
        }
    }
    out
}

/// `smp://` relays and `xftp://` file servers (ADR-0033).
pub(super) fn relays(
    r: &mut Reader<'_>,
    s: &mut Section,
    key: &str,
    _default: Vec<String>,
) -> Vec<String> {
    let mut out = Vec::new();
    for (i, v) in r.list(s, key).into_iter().enumerate() {
        match v {
            Value::String(a)
                if (a.starts_with("smp://") || a.starts_with("xftp://"))
                    && !a.chars().any(char::is_whitespace) =>
            {
                out.push(a);
            }
            _ => r.dropped(
                s,
                key,
                i,
                "not an smp:// relay or xftp:// file server address",
            ),
        }
    }
    out
}

pub(super) fn search_dirs(
    r: &mut Reader<'_>,
    s: &mut Section,
    key: &str,
    _default: Vec<PathBuf>,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for (i, v) in r.list(s, key).into_iter().enumerate() {
        let Value::String(p) = v else {
            r.dropped(s, key, i, "not a string");
            continue;
        };
        let path = PathBuf::from(&p);
        let normal = crate::paths::normalize(&path);
        if !path.is_absolute() {
            r.dropped(s, key, i, "not an absolute path");
        } else if SEARCH_DIR_DENY.iter().any(|d| normal == Path::new(d)) {
            r.dropped(s, key, i, "a system directory");
        } else if out.len() >= MAX_EXTRA_SEARCH_DIRS {
            r.dropped(
                s,
                key,
                i,
                format!("more than {MAX_EXTRA_SEARCH_DIRS} entries"),
            );
        } else if !out.contains(&normal) {
            out.push(normal);
        }
    }
    out
}

/// Needs `r.developer_mode`, so `[advanced]` is read first.
pub(super) fn global_env(
    r: &mut Reader<'_>,
    s: &mut Section,
    key: &str,
    _default: Vec<EnvVar>,
) -> Vec<EnvVar> {
    let mut out: Vec<EnvVar> = Vec::new();
    for (i, v) in r.list(s, key).into_iter().enumerate() {
        let Ok(var) = v.try_into::<EnvVar>() else {
            r.dropped(s, key, i, "expected { name = \"…\", value = \"…\" }");
            continue;
        };
        if let Err(e) = var.validate() {
            r.dropped(s, key, i, format!("{:?}", e.problem));
            continue;
        }
        let rule = r.opts.env_rule.map_or(EnvRule::Allowed, |f| f(&var.name));
        if rule == EnvRule::Reserved {
            r.dropped(s, key, i, format!("{} is reserved", var.name));
        } else if rule == EnvRule::Dangerous && !r.developer_mode {
            r.dropped(s, key, i, format!("{} requires Developer Mode", var.name));
        } else if out.len() >= MAX_GLOBAL_ENV {
            r.dropped(s, key, i, format!("more than {MAX_GLOBAL_ENV} entries"));
        } else if out.iter().any(|e| e.name == var.name) {
            r.dropped(s, key, i, format!("{} is set twice", var.name));
        } else {
            out.push(var);
        }
    }
    out
}

/// An override of `umu-run` or `bwrap`: empty = detect; otherwise an executable file.
pub(super) fn executable(
    r: &mut Reader<'_>,
    s: &mut Section,
    key: &str,
    _default: Option<PathBuf>,
) -> Option<PathBuf> {
    let text = r.string(s, key)?;
    if text.is_empty() {
        return None;
    }
    let p = PathBuf::from(&text);
    if !p.is_absolute() {
        r.invalid(s, key, "must be an absolute path");
        return None;
    }
    match r.fs.metadata_follow(&p) {
        Ok(m) if m.is_file() && m.mode & 0o111 != 0 => Some(p),
        Ok(_) => {
            r.invalid(s, key, "not an executable file");
            None
        }
        Err(e) => {
            r.invalid(s, key, format!("cannot be used: {:?}", e.kind));
            None
        }
    }
}
