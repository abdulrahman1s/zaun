//! bubblewrap discovery, option parsing and the namespace/overlay probes
//! (BUBBLEWRAP_SANDBOX §10, ERROR_HANDLING §4.4).

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::exec::{find_in, is_executable, path_dirs};
use super::facts::{
    BwrapFacts, BwrapFlag, HostHint, HostProblem, KernelFacts, OsRelease, ToolVersion,
};
use super::runner::{CommandOutput, CommandRunner, CommandSpec};
use crate::error::ErrorCode;
use crate::fsx::Fs;

/// Fixed directories searched before `PATH`.
pub const BWRAP_SEARCH_DIRS: &[&str] = &[
    "/usr/bin",
    "/bin",
    "/usr/local/bin",
    "/run/current-system/sw/bin",
];

/// The AppArmor profile shipped in the `.deb` (OQ-5, `PKG-006`).
pub const APPARMOR_PROFILE: &str = "/etc/apparmor.d/easyplay";

const STDERR_DETAIL_CAP: usize = 2048;

/// Find `bwrap`: the fixed directories, then each directory of `PATH`.
pub fn locate_bwrap(fs: &dyn Fs, path_env: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = BWRAP_SEARCH_DIRS.iter().map(PathBuf::from).collect();
    dirs.extend(path_dirs(path_env));
    find_in(fs, &dirs, "bwrap")
}

/// Parse `bubblewrap 0.12.0`.
pub fn parse_version(text: &str) -> Option<ToolVersion> {
    let line = text.lines().find(|l| !l.trim().is_empty())?;
    let v = line.trim().strip_prefix("bubblewrap")?.trim();
    v.parse().ok()
}

/// Every known option listed by `bwrap --help` (lines whose first token starts with `--`).
pub fn parse_help(text: &str) -> BTreeSet<BwrapFlag> {
    text.lines()
        .filter_map(|l| {
            let tok = l.split_whitespace().next()?;
            tok.starts_with("--")
                .then(|| BwrapFlag::parse(tok))
                .flatten()
        })
        .collect()
}

/// Read the user-namespace switches from `/proc/sys`.
pub fn read_kernel_facts(fs: &dyn Fs) -> KernelFacts {
    let num = |p: &str| -> Option<u64> {
        fs.read_follow(Path::new(p), 64)
            .ok()
            .and_then(|b| String::from_utf8_lossy(&b).trim().parse().ok())
    };
    KernelFacts {
        unprivileged_userns_clone: num("/proc/sys/kernel/unprivileged_userns_clone"),
        max_user_namespaces: num("/proc/sys/user/max_user_namespaces"),
        apparmor_restrict_unprivileged_userns: num(
            "/proc/sys/kernel/apparmor_restrict_unprivileged_userns",
        ),
    }
}

/// Guidance for a failed user-namespace test, from the kernel switches, the distribution and
/// bwrap's message.
pub fn userns_hint(os: &OsRelease, kernel: &KernelFacts, stderr: &str) -> Option<HostHint> {
    if kernel.unprivileged_userns_clone == Some(0) {
        return Some(HostHint::Sysctl {
            setting: "kernel.unprivileged_userns_clone=1".into(),
        });
    }
    if kernel.max_user_namespaces == Some(0) {
        return Some(HostHint::Sysctl {
            setting: "user.max_user_namespaces=28633".into(),
        });
    }
    let apparmor = kernel.apparmor_restrict_unprivileged_userns == Some(1)
        || (os.is_like("ubuntu") && stderr.contains("Permission denied"));
    if apparmor {
        return Some(HostHint::AppArmor {
            sysctl: "kernel.apparmor_restrict_unprivileged_userns=0".into(),
            profile: APPARMOR_PROFILE.into(),
        });
    }
    None
}

fn detail(cmd: &CommandSpec, out: Option<&CommandOutput>, err: Option<&str>) -> String {
    let mut s = format!("$ {}", cmd.display());
    if let Some(out) = out {
        if out.timed_out {
            s.push_str("\n(timed out)");
        } else {
            s.push_str(&format!("\nexit status {}", out.status));
        }
        let stderr = out.stderr_text();
        let stderr = stderr.trim();
        if !stderr.is_empty() {
            s.push('\n');
            s.extend(stderr.chars().take(STDERR_DETAIL_CAP));
        }
    }
    if let Some(e) = err {
        s.push('\n');
        s.push_str(e);
    }
    s
}

/// Inputs of the bwrap probe.
#[derive(Clone, Debug)]
pub struct BwrapProbeInput<'a> {
    /// Host `PATH` (searched after the fixed directories).
    pub host_path: Option<&'a std::ffi::OsStr>,
    /// `PATH` for the probe commands, so that `true` resolves on the host layout.
    pub probe_path: &'a str,
    /// Distribution, for hints.
    pub os: &'a OsRelease,
    /// Kernel switches, for hints.
    pub kernel: &'a KernelFacts,
    /// An existing directory used as the overlay test's lower layer (never written: writes go
    /// to an invisible tmpfs).
    pub overlay_dir: &'a Path,
}

/// The three probe argument vectors (after the bwrap path).
pub fn userns_args() -> Vec<OsString> {
    ["--unshare-user", "--ro-bind", "/", "/", "true"]
        .map(OsString::from)
        .to_vec()
}

/// `bwrap --unshare-user --unshare-pid --ro-bind / / --dev /dev --proc /proc -- bwrap
/// --unshare-user --ro-bind / / true`.
pub fn nested_args(bwrap: &Path) -> Vec<OsString> {
    let mut v: Vec<OsString> = [
        "--unshare-user",
        "--unshare-pid",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--",
    ]
    .map(OsString::from)
    .to_vec();
    v.push(bwrap.as_os_str().to_owned());
    v.extend(userns_args());
    v
}

/// `bwrap --unshare-user --ro-bind / / --overlay-src DIR --tmp-overlay DIR true`.
pub fn overlay_args(dir: &Path) -> Vec<OsString> {
    let mut v: Vec<OsString> = ["--unshare-user", "--ro-bind", "/", "/", "--overlay-src"]
        .map(OsString::from)
        .to_vec();
    v.push(dir.as_os_str().to_owned());
    v.push("--tmp-overlay".into());
    v.push(dir.as_os_str().to_owned());
    v.push("true".into());
    v
}

/// Probe bwrap at `path`: version, options, setuid, namespaces, overlay. Problems are
/// appended to `problems`.
pub fn probe_bwrap_at(
    fs: &dyn Fs,
    runner: &dyn CommandRunner,
    path: &Path,
    input: &BwrapProbeInput<'_>,
    problems: &mut Vec<HostProblem>,
) -> BwrapFacts {
    let spec = |args: Vec<OsString>| {
        let mut c = CommandSpec::new(path, &args);
        c = c.env("PATH", input.probe_path);
        c
    };
    let run = |c: &CommandSpec| runner.run(c);

    let version_cmd = spec(vec!["--version".into()]);
    let version = run(&version_cmd)
        .ok()
        .and_then(|o| parse_version(&o.stdout_text()));

    let help_cmd = spec(vec!["--help".into()]);
    let flags = match run(&help_cmd) {
        // `--help` exits 0 and prints to stdout; older builds print usage to stderr.
        Ok(o) => {
            let mut text = o.stdout_text();
            text.push_str(&o.stderr_text());
            parse_help(&text)
        }
        Err(_) => BTreeSet::new(),
    };

    let setuid = fs
        .metadata_follow(path)
        .is_ok_and(|m| m.mode & 0o4000 != 0 && m.uid == 0);

    let mut facts = BwrapFacts {
        path: path.to_path_buf(),
        version,
        flags,
        setuid,
        userns_ok: false,
        nested_userns_ok: false,
        overlay_ok: false,
    };

    let missing = facts.missing_required();
    if !missing.is_empty() {
        let names: Vec<&str> = missing.iter().map(|f| f.as_str()).collect();
        problems.push(HostProblem::new(
            ErrorCode::HostBwrapMissingOption,
            format!(
                "{} {} does not support: {}",
                path.display(),
                facts.version.map(|v| v.to_string()).unwrap_or_default(),
                names.join(", ")
            ),
        ));
    }

    // User namespaces.
    let userns_cmd = spec(userns_args());
    match run(&userns_cmd) {
        Ok(o) if o.success() => facts.userns_ok = true,
        result => {
            let (out, err) = match &result {
                Ok(o) => (Some(o), None),
                Err(e) => (None, Some(e.to_string())),
            };
            let stderr = out.map(CommandOutput::stderr_text).unwrap_or_default();
            let (code, hint) = if setuid {
                (ErrorCode::HostBwrapSetuidOnly, Some(HostHint::SetuidBwrap))
            } else {
                (
                    ErrorCode::HostUsernsUnavailable,
                    userns_hint(input.os, input.kernel, &stderr),
                )
            };
            problems.push(HostProblem {
                code,
                detail: detail(&userns_cmd, out, err.as_deref()),
                hint,
            });
        }
    }

    if facts.userns_ok {
        let nested_cmd = spec(nested_args(path));
        match run(&nested_cmd) {
            Ok(o) if o.success() => facts.nested_userns_ok = true,
            result => {
                let (out, err) = match &result {
                    Ok(o) => (Some(o), None),
                    Err(e) => (None, Some(e.to_string())),
                };
                let stderr = out.map(CommandOutput::stderr_text).unwrap_or_default();
                problems.push(HostProblem {
                    code: ErrorCode::HostNestedUsernsUnavailable,
                    detail: detail(&nested_cmd, out, err.as_deref()),
                    hint: userns_hint(input.os, input.kernel, &stderr),
                });
            }
        }

        let overlay_supported =
            facts.has(BwrapFlag::OverlaySrc) && facts.has(BwrapFlag::TmpOverlay) && !facts.setuid;
        if overlay_supported
            && fs
                .metadata_follow(input.overlay_dir)
                .is_ok_and(|m| m.is_dir())
        {
            let overlay_cmd = spec(overlay_args(input.overlay_dir));
            facts.overlay_ok = run(&overlay_cmd).is_ok_and(|o| o.success());
        }
    }
    facts
}

/// Locate and probe bwrap. `None` (with `EP-HOST-001`) when no binary is found.
pub fn probe_bwrap(
    fs: &dyn Fs,
    runner: &dyn CommandRunner,
    input: &BwrapProbeInput<'_>,
    problems: &mut Vec<HostProblem>,
) -> Option<BwrapFacts> {
    match locate_bwrap(fs, input.host_path) {
        Some(path) => Some(probe_bwrap_at(fs, runner, &path, input, problems)),
        None => {
            let mut searched: Vec<String> =
                BWRAP_SEARCH_DIRS.iter().map(|s| (*s).to_owned()).collect();
            searched.push("$PATH".into());
            problems.push(HostProblem::new(
                ErrorCode::HostBwrapNotFound,
                format!("bwrap not found in {}", searched.join(", ")),
            ));
            None
        }
    }
}

/// Whether `path` is still an executable bwrap (cheap revalidation).
pub fn still_present(fs: &dyn Fs, path: &Path) -> bool {
    is_executable(fs, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELP_0_12: &str = include_str!("../../tests/data/bwrap/help-0.12.0.txt");
    const HELP_0_13: &str = include_str!("../../tests/data/bwrap/help-0.13.1.txt");
    const HELP_0_11: &str = include_str!("../../tests/data/bwrap/help-0.11.0.txt");
    const HELP_0_9: &str = include_str!("../../tests/data/bwrap/help-0.9.0.txt");
    const HELP_0_8: &str = include_str!("../../tests/data/bwrap/help-0.8.0.txt");

    fn facts(help: &str) -> BwrapFacts {
        BwrapFacts {
            path: "/usr/bin/bwrap".into(),
            version: None,
            flags: parse_help(help),
            setuid: false,
            userns_ok: true,
            nested_userns_ok: true,
            overlay_ok: false,
        }
    }

    #[test]
    fn versions() {
        assert_eq!(
            parse_version("bubblewrap 0.12.0\n"),
            Some(ToolVersion::new(0, 12, 0))
        );
        assert_eq!(
            parse_version("bubblewrap 0.8.0"),
            Some(ToolVersion::new(0, 8, 0))
        );
        assert_eq!(
            parse_version("bubblewrap 0.11.0.1\n"),
            Some(ToolVersion::new(0, 11, 0))
        );
        assert_eq!(parse_version("nonsense"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn captured_help_outputs_have_every_required_option() {
        for (name, help) in [
            ("0.8", HELP_0_8),
            ("0.9", HELP_0_9),
            ("0.11", HELP_0_11),
            ("0.12", HELP_0_12),
            ("0.13", HELP_0_13),
        ] {
            let f = facts(help);
            assert!(
                f.missing_required().is_empty(),
                "{name}: {:?}",
                f.missing_required()
            );
            assert!(f.has(BwrapFlag::DisableUserns), "{name}");
        }
    }

    #[test]
    fn overlay_options_appear_in_0_10() {
        for help in [HELP_0_8, HELP_0_9] {
            assert!(!facts(help).has(BwrapFlag::OverlaySrc));
            assert!(!facts(help).has(BwrapFlag::TmpOverlay));
        }
        for help in [HELP_0_11, HELP_0_12, HELP_0_13] {
            let f = facts(help);
            assert!(f.has(BwrapFlag::OverlaySrc) && f.has(BwrapFlag::TmpOverlay));
            assert!(f.has(BwrapFlag::Overlay) && f.has(BwrapFlag::RoOverlay));
        }
        // 0.12 lists every option EasyPlay knows.
        assert_eq!(facts(HELP_0_12).flags.len(), BwrapFlag::ALL.len());
    }

    #[test]
    fn missing_option_is_named() {
        let help = HELP_0_12
            .lines()
            .filter(|l| !l.trim_start().starts_with("--json-status-fd"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            facts(&help).missing_required(),
            vec![BwrapFlag::JsonStatusFd]
        );
    }

    #[test]
    fn hints() {
        let os = OsRelease {
            id: Some("ubuntu".into()),
            id_like: vec!["debian".into()],
            ..OsRelease::default()
        };
        let k = KernelFacts {
            apparmor_restrict_unprivileged_userns: Some(1),
            ..KernelFacts::default()
        };
        assert!(matches!(
            userns_hint(&os, &k, ""),
            Some(HostHint::AppArmor { .. })
        ));
        let k = KernelFacts {
            unprivileged_userns_clone: Some(0),
            ..KernelFacts::default()
        };
        assert_eq!(
            userns_hint(&OsRelease::default(), &k, ""),
            Some(HostHint::Sysctl {
                setting: "kernel.unprivileged_userns_clone=1".into()
            })
        );
        assert_eq!(
            userns_hint(&OsRelease::default(), &KernelFacts::default(), ""),
            None
        );
    }
}
