//! The key-by-key reader behind `from_table`: a bad value warns and yields the default (CONFIGURATION §2.1).

use std::str::FromStr;

use toml::Value;

use super::load::{ConfigProblem, ConfigWarning, LoadOptions};
use crate::fsx::Fs;

pub(super) struct Reader<'a> {
    pub(super) fs: &'a dyn Fs,
    pub(super) opts: &'a LoadOptions<'a>,
    /// `advanced.developer_mode`: `[advanced]` is read before `[launch]`, whose
    /// `global_env` depends on it.
    pub(super) developer_mode: bool,
    pub(super) warnings: Vec<ConfigWarning>,
    pub(super) unknown: toml::Table,
}

/// One `[section]` being read; keys are removed as they are consumed.
pub(super) struct Section {
    name: &'static str,
    table: toml::Table,
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::String(_) => "a string",
        Value::Integer(_) => "an integer",
        Value::Float(_) => "a number",
        Value::Boolean(_) => "true or false",
        Value::Datetime(_) => "a date",
        Value::Array(_) => "a list",
        Value::Table(_) => "a table",
    }
}

/// The words of an enumeration, for the "not one of" reason.
pub(super) fn names<T: Copy>(all: &[T], f: impl Fn(T) -> &'static str) -> Vec<&'static str> {
    all.iter().map(|v| f(*v)).collect()
}

impl<'a> Reader<'a> {
    pub(super) fn new(fs: &'a dyn Fs, opts: &'a LoadOptions<'a>) -> Self {
        Self {
            fs,
            opts,
            developer_mode: false,
            warnings: Vec::new(),
            unknown: toml::Table::new(),
        }
    }

    fn warn(&mut self, sec: &Section, key: &str, problem: ConfigProblem) {
        self.warnings.push(ConfigWarning {
            key: format!("{}.{key}", sec.name),
            problem,
        });
    }

    pub(super) fn invalid(&mut self, sec: &Section, key: &str, reason: impl Into<String>) {
        let reason = reason.into();
        self.warn(sec, key, ConfigProblem::InvalidValue { reason });
    }

    fn expected(&mut self, sec: &Section, key: &str, what: &str, found: &Value) {
        let found = type_name(found);
        self.invalid(sec, key, format!("expected {what}, found {found}"));
    }

    pub(super) fn dropped(
        &mut self,
        sec: &Section,
        key: &str,
        index: usize,
        reason: impl Into<String>,
    ) {
        let reason = reason.into();
        self.warn(sec, key, ConfigProblem::DroppedEntry { index, reason });
    }

    /// Take `[name]` out of the root; a non-table is a warning and reads as empty.
    pub(super) fn section(&mut self, root: &mut toml::Table, name: &'static str) -> Section {
        let table = match root.remove(name) {
            None => toml::Table::new(),
            Some(Value::Table(t)) => t,
            Some(other) => {
                let reason = format!("expected a table, found {}", type_name(&other));
                self.warnings.push(ConfigWarning {
                    key: name.into(),
                    problem: ConfigProblem::InvalidValue { reason },
                });
                toml::Table::new()
            }
        };
        Section { name, table }
    }

    /// Keys left in the section are unknown ones, kept for rewrite.
    pub(super) fn finish(&mut self, sec: Section) {
        if !sec.table.is_empty() {
            self.unknown
                .insert(sec.name.into(), Value::Table(sec.table));
        }
    }

    pub(super) fn bool(&mut self, sec: &mut Section, key: &str, default: bool) -> bool {
        match sec.table.remove(key) {
            None => default,
            Some(Value::Boolean(b)) => b,
            Some(v) => {
                self.expected(sec, key, "true or false", &v);
                default
            }
        }
    }

    pub(super) fn word<T: FromStr + Copy>(
        &mut self,
        sec: &mut Section,
        key: &str,
        default: T,
        allowed: &[&str],
    ) -> T {
        match sec.table.remove(key) {
            None => default,
            Some(Value::String(s)) => s.trim().parse().unwrap_or_else(|_| {
                let allowed = allowed.join(", ");
                self.invalid(sec, key, format!("{s:?} is not one of {allowed}"));
                default
            }),
            Some(v) => {
                self.expected(sec, key, "a string", &v);
                default
            }
        }
    }

    pub(super) fn int(
        &mut self,
        sec: &mut Section,
        key: &str,
        default: i64,
        lo: i64,
        hi: i64,
    ) -> i64 {
        match sec.table.remove(key) {
            None => default,
            Some(Value::Integer(i)) if (lo..=hi).contains(&i) => i,
            Some(Value::Integer(i)) => {
                let to = i.clamp(lo, hi);
                self.warn(sec, key, ConfigProblem::Clamped { to: to.to_string() });
                to
            }
            Some(v) => {
                self.expected(sec, key, "an integer", &v);
                default
            }
        }
    }

    pub(super) fn float(
        &mut self,
        sec: &mut Section,
        key: &str,
        default: f64,
        lo: f64,
        hi: f64,
    ) -> f64 {
        let v = match sec.table.remove(key) {
            None => return default,
            Some(Value::Float(f)) => f,
            #[allow(clippy::cast_precision_loss)]
            Some(Value::Integer(i)) => i as f64,
            Some(v) => {
                self.expected(sec, key, "a number", &v);
                return default;
            }
        };
        if v.is_nan() {
            self.invalid(sec, key, "not a number");
            return default;
        }
        if (lo..=hi).contains(&v) {
            return v;
        }
        let to = v.clamp(lo, hi);
        self.warn(sec, key, ConfigProblem::Clamped { to: to.to_string() });
        to
    }

    pub(super) fn string(&mut self, sec: &mut Section, key: &str) -> Option<String> {
        match sec.table.remove(key) {
            None => None,
            Some(Value::String(s)) => Some(s),
            Some(v) => {
                self.expected(sec, key, "a string", &v);
                None
            }
        }
    }

    pub(super) fn list(&mut self, sec: &mut Section, key: &str) -> Vec<Value> {
        match sec.table.remove(key) {
            None => Vec::new(),
            Some(Value::Array(a)) => a,
            Some(v) => {
                self.expected(sec, key, "a list", &v);
                Vec::new()
            }
        }
    }
}
