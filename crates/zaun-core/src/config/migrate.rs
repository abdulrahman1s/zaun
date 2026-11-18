//! Version-to-version upgrades of `config.toml` (LOGIC_SPECS §15, ADR-0011 rule 6).
//!
//! A chain of pure functions on the parsed TOML table; `load` keeps the original file as
//! `config.toml.bak-v<n>` and rewrites the upgraded one. Empty while
//! `CONFIG_SCHEMA_VERSION = 1`.

use std::path::{Path, PathBuf};

use crate::consts::CONFIG_SCHEMA_VERSION;

/// One step: the table of version `n` to version `n + 1`.
pub type Step = fn(toml::Table) -> Result<toml::Table, String>;

/// Steps; index `i` upgrades version `FIRST + i`.
const STEPS: &[Step] = &[];
/// The oldest version with an upgrade path.
const FIRST: u32 = 1;

/// Upgrade `table` from version `from` to [`CONFIG_SCHEMA_VERSION`].
pub fn upgrade(table: toml::Table, from: u32) -> Result<toml::Table, String> {
    upgrade_with(table, from, CONFIG_SCHEMA_VERSION, FIRST, STEPS)
}

/// The generic chain (tests use synthetic steps).
pub fn upgrade_with(
    mut table: toml::Table,
    from: u32,
    current: u32,
    first: u32,
    steps: &[Step],
) -> Result<toml::Table, String> {
    if from > current {
        return Err(format!("version {from} is newer than {current}"));
    }
    if from < first {
        return Err(format!("version {from} has no upgrade path"));
    }
    let mut v = from;
    while v < current {
        let idx = usize::try_from(v - first).map_err(|e| e.to_string())?;
        let step = steps
            .get(idx)
            .ok_or_else(|| format!("no upgrade step from version {v}"))?;
        table = step(table)?;
        v += 1;
        table.insert("schema_version".into(), toml::Value::Integer(i64::from(v)));
    }
    Ok(table)
}

/// `config.toml.bak-v<from>` next to `path`.
pub fn backup_path(path: &Path, from: u32) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(format!(".bak-v{from}"));
    PathBuf::from(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic v0: `[ui] theme` became `[appearance] theme`.
    fn v0_to_v1(mut t: toml::Table) -> Result<toml::Table, String> {
        if let Some(toml::Value::Table(mut ui)) = t.remove("ui")
            && let Some(theme) = ui.remove("theme")
        {
            let mut appearance = toml::Table::new();
            appearance.insert("theme".into(), theme);
            t.insert("appearance".into(), toml::Value::Table(appearance));
        }
        Ok(t)
    }

    #[test]
    fn synthetic_v0_upgrades_to_v1() {
        let v0: toml::Table = "schema_version = 0\n[ui]\ntheme = \"dark\"\n"
            .parse()
            .unwrap();
        let v1 = upgrade_with(v0, 0, 1, 0, &[v0_to_v1]).unwrap();
        assert_eq!(v1["schema_version"].as_integer(), Some(1));
        assert_eq!(v1["appearance"]["theme"].as_str(), Some("dark"));
        assert!(!v1.contains_key("ui"));
        assert!(upgrade_with(toml::Table::new(), 0, 1, 1, &[]).is_err());
        assert!(upgrade_with(toml::Table::new(), 2, 1, 1, &[]).is_err());
        assert!(upgrade(toml::Table::new(), CONFIG_SCHEMA_VERSION).is_ok());
    }

    #[test]
    fn backup_name() {
        assert_eq!(
            backup_path(Path::new("/c/config.toml"), 0),
            PathBuf::from("/c/config.toml.bak-v0")
        );
    }
}
