//! Reading versioned JSON documents: the sandbox policy and the launch configuration
//! (LOGIC_SPECS §15, ADR-0011 rule 5).
//!
//! Every stored document is read through [`parse_policy`] or [`parse_launch_config`]: they
//! upgrade older versions in memory by chaining pure `vN → vN+1` steps on the JSON value,
//! refuse documents newer than this build ([`DocError::TooNew`]) and report anything that
//! does not parse ([`DocError::Unparsable`]). Unknown fields are ignored on read and dropped
//! on the next rewrite. The upgraded document is persisted only when the game is next saved.

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{LaunchConfig, SandboxPolicy};
use crate::consts::{LAUNCH_SCHEMA_VERSION, POLICY_SCHEMA_VERSION};

/// Why a stored document cannot be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DocError {
    /// Written by a newer Zaun.
    #[error("document version {found} is newer than the supported {supported}")]
    TooNew {
        /// The version found.
        found: u32,
        /// The newest version this build reads.
        supported: u32,
    },
    /// Not valid JSON, the wrong shape, or a version without an upgrade path.
    #[error("document cannot be read: {0}")]
    Unparsable(String),
}

/// One upgrade step: the document of version `n` to version `n + 1`.
pub type UpgradeStep = fn(Value) -> Result<Value, DocError>;

/// Policy upgrade steps; index `i` upgrades version `i + 1` to `i + 2`. Empty while
/// `POLICY_SCHEMA_VERSION = 1`.
const POLICY_STEPS: &[UpgradeStep] = &[];
/// Launch-configuration upgrade steps, indexed like [`POLICY_STEPS`].
const LAUNCH_STEPS: &[UpgradeStep] = &[];

/// Parse a policy document stored with version `schema` (`games.policy_schema`).
pub fn parse_policy(json: &str, schema: u32) -> Result<SandboxPolicy, DocError> {
    parse_versioned(json, schema, POLICY_SCHEMA_VERSION, 1, POLICY_STEPS)
}

/// Parse a launch-configuration document stored with version `schema`
/// (`games.launch_schema`).
pub fn parse_launch_config(json: &str, schema: u32) -> Result<LaunchConfig, DocError> {
    parse_versioned(json, schema, LAUNCH_SCHEMA_VERSION, 1, LAUNCH_STEPS)
}

/// The generic chain: `steps[i]` upgrades version `first + i` to `first + i + 1`.
pub(crate) fn parse_versioned<T: DeserializeOwned>(
    json: &str,
    schema: u32,
    current: u32,
    first: u32,
    steps: &[UpgradeStep],
) -> Result<T, DocError> {
    if schema > current {
        return Err(DocError::TooNew {
            found: schema,
            supported: current,
        });
    }
    if schema < first {
        return Err(DocError::Unparsable(format!(
            "version {schema} has no upgrade path"
        )));
    }
    let mut value: Value =
        serde_json::from_str(json).map_err(|e| DocError::Unparsable(e.to_string()))?;
    if let Some(inner) = value.get("schema_version").and_then(Value::as_u64)
        && inner > u64::from(current)
    {
        return Err(DocError::TooNew {
            found: u32::try_from(inner).unwrap_or(u32::MAX),
            supported: current,
        });
    }
    let mut version = schema;
    while version < current {
        let idx = usize::try_from(version - first).unwrap_or(usize::MAX);
        let step = steps.get(idx).ok_or_else(|| {
            DocError::Unparsable(format!("no upgrade step from version {version}"))
        })?;
        value = step(value)?;
        version += 1;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("schema_version".into(), Value::from(version));
        }
    }
    serde_json::from_value(value).map_err(|e| DocError::Unparsable(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct V2 {
        schema_version: u32,
        name: String,
        level: u32,
    }

    fn v0_to_v1(mut v: Value) -> Result<Value, DocError> {
        let obj = v
            .as_object_mut()
            .ok_or_else(|| DocError::Unparsable("not an object".into()))?;
        let title = obj
            .remove("title")
            .ok_or_else(|| DocError::Unparsable("no title".into()))?;
        obj.insert("name".into(), title);
        Ok(v)
    }

    fn v1_to_v2(mut v: Value) -> Result<Value, DocError> {
        if let Some(obj) = v.as_object_mut() {
            obj.insert("level".into(), Value::from(3));
        }
        Ok(v)
    }

    const STEPS: &[UpgradeStep] = &[v0_to_v1, v1_to_v2];

    #[test]
    fn synthetic_v0_upgrades_through_the_chain() {
        let got: V2 = parse_versioned(r#"{"schema_version":0,"title":"x"}"#, 0, 2, 0, STEPS)
            .expect("upgrade");
        assert_eq!(
            got,
            V2 {
                schema_version: 2,
                name: "x".into(),
                level: 3
            }
        );
        let current: V2 = parse_versioned(
            r#"{"schema_version":2,"name":"y","level":1,"unknown":true}"#,
            2,
            2,
            0,
            STEPS,
        )
        .expect("current");
        assert_eq!(current.level, 1);
    }

    #[test]
    fn too_new_and_garbage_are_distinct() {
        assert_eq!(
            parse_versioned::<V2>("{}", 3, 2, 0, STEPS),
            Err(DocError::TooNew {
                found: 3,
                supported: 2
            })
        );
        assert!(matches!(
            parse_versioned::<V2>("not json", 2, 2, 0, STEPS),
            Err(DocError::Unparsable(_))
        ));
        assert!(matches!(
            parse_versioned::<V2>(r#"{"schema_version":0}"#, 0, 2, 0, STEPS),
            Err(DocError::Unparsable(_))
        ));
        // The column says 1 but the document claims a newer version.
        assert!(matches!(
            parse_policy(r#"{"schema_version":9}"#, 1),
            Err(DocError::TooNew { found: 9, .. })
        ));
        assert!(matches!(
            parse_policy("{}", 2),
            Err(DocError::TooNew { found: 2, .. })
        ));
        assert!(matches!(
            parse_launch_config("[]", 1),
            Err(DocError::Unparsable(_))
        ));
    }

    #[test]
    fn current_documents_parse() {
        let launch = LaunchConfig::default();
        let json = serde_json::to_string(&launch).unwrap();
        assert_eq!(parse_launch_config(&json, 1), Ok(launch));
        let policy = super::super::tests::example_policy();
        let json = serde_json::to_string(&policy).unwrap();
        assert_eq!(parse_policy(&json, 1), Ok(policy));
    }
}
