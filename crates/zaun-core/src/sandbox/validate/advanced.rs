//! Developer-only ("unsafe") options a game carries (DEVELOPER_MODE §1.4, §7; DEV-006).
//!
//! Adding them requires Developer Mode: validation refuses them otherwise, in the editors
//! and for imported settings. Options already saved stay valid when the mode is turned off:
//! the launch validates a game that carries them as Developer Mode would
//! ([`effective_developer_mode`]), and the game page lists them under "uses advanced options"
//! with *Review*.

use crate::model::{LaunchConfig, PolicyException, SandboxPolicy};

use super::{UserVarRule, classify_user_var};

/// One developer-only option a game uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdvancedOption {
    /// An `ExtraReadOnlyPath` exception.
    ExtraReadOnlyPath(std::path::PathBuf),
    /// An `ExtraDevice` exception.
    ExtraDevice(std::path::PathBuf),
    /// A `PassEnv` exception of a variable classed dangerous.
    PassDangerousEnv(String),
    /// A dangerous variable set in the launch options.
    DangerousEnv(String),
    /// Wine debug channels.
    WineDebug(String),
}

impl AdvancedOption {
    /// A short English description (the Review list).
    pub fn describe(&self) -> String {
        match self {
            AdvancedOption::ExtraReadOnlyPath(p) => {
                format!("Extra read-only folder {}", p.display())
            }
            AdvancedOption::ExtraDevice(p) => {
                format!("Direct access to the device {}", p.display())
            }
            AdvancedOption::PassDangerousEnv(n) => format!("Copies {n} from your session"),
            AdvancedOption::DangerousEnv(n) => format!("Sets {n}"),
            AdvancedOption::WineDebug(c) => format!("Wine debug channels {c}"),
        }
    }
}

/// The developer-only options in a game's settings, in a stable order.
pub fn advanced_options(policy: &SandboxPolicy, launch: &LaunchConfig) -> Vec<AdvancedOption> {
    let mut out = Vec::new();
    for e in &policy.exceptions {
        match e {
            PolicyException::ExtraReadOnlyPath { path, .. } => {
                out.push(AdvancedOption::ExtraReadOnlyPath(path.clone()));
            }
            PolicyException::ExtraDevice { path, .. } => {
                out.push(AdvancedOption::ExtraDevice(path.clone()));
            }
            PolicyException::PassEnv { name, .. }
                if classify_user_var(name) == UserVarRule::Dangerous =>
            {
                out.push(AdvancedOption::PassDangerousEnv(name.clone()));
            }
            PolicyException::PassEnv { .. } => {}
        }
    }
    for v in &launch.env {
        if classify_user_var(&v.name) == UserVarRule::Dangerous {
            out.push(AdvancedOption::DangerousEnv(v.name.clone()));
        }
    }
    if let Some(d) = launch.compat.wine_debug.as_ref().filter(|d| !d.is_empty()) {
        out.push(AdvancedOption::WineDebug(d.clone()));
    }
    out
}

/// Developer Mode as the launch sees it: on when the preference is on, and for a game whose
/// saved settings already carry developer-only options (they were accepted when saved and
/// keep working when the mode is turned off).
pub fn effective_developer_mode(
    preference: bool,
    policy: &SandboxPolicy,
    launch: &LaunchConfig,
) -> bool {
    preference || !advanced_options(policy, launch).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EnvVar;

    #[test]
    fn lists_only_developer_options() {
        let mut p = crate::sandbox::offline();
        let mut l = LaunchConfig::default();
        assert!(advanced_options(&p, &l).is_empty());
        assert!(!effective_developer_mode(false, &p, &l));
        l.env.push(EnvVar {
            name: "DXVK_HUD".into(),
            value: "1".into(),
        });
        assert!(advanced_options(&p, &l).is_empty());
        l.env.push(EnvVar {
            name: "LD_PRELOAD".into(),
            value: "/x.so".into(),
        });
        p.exceptions.push(PolicyException::ExtraDevice {
            path: "/dev/hidraw0".into(),
            reason: "flight stick firmware".into(),
        });
        p.exceptions.push(PolicyException::PassEnv {
            name: "MANGOHUD_CONFIG".into(),
            reason: "overlay settings".into(),
        });
        l.compat.wine_debug = Some("+relay".into());
        let a = advanced_options(&p, &l);
        assert_eq!(
            a,
            vec![
                AdvancedOption::ExtraDevice("/dev/hidraw0".into()),
                AdvancedOption::DangerousEnv("LD_PRELOAD".into()),
                AdvancedOption::WineDebug("+relay".into()),
            ]
        );
        assert!(effective_developer_mode(false, &p, &l));
        assert!(a.iter().all(|o| !o.describe().is_empty()));
    }
}
