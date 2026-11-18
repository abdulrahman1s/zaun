//! Locate `umu-run`, compute the closure that must be visible in the sandbox, detect
//! wrappers, and read its version (UMU_INTEGRATION §1–2).
//!
//! The locating algorithm is pure filesystem work and lives here (layer 3) so that the host
//! probe can run it; `umu::locate` re-exports it for the umu module's callers.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::exec::{find_in, is_executable, path_dirs};
use super::facts::{HostProblem, ToolVersion, UmuFacts, UmuWrapper};
use super::runner::{CommandRunner, CommandSpec};
use crate::error::ErrorCode;
use crate::fsx::Fs;

/// Fixed candidate locations, after the `paths.umu_run` preference and before `PATH`.
pub const UMU_SEARCH: &[&str] = &[
    "/usr/bin/umu-run",
    "/usr/local/bin/umu-run",
    "/run/current-system/sw/bin/umu-run",
];

const SCRIPT_CAP: u64 = 256 * 1024;
const MAX_SHELL_ADDITIONS: usize = 8;
const MAX_CHAIN: usize = 6;
const SHELLS: &[&str] = &["sh", "bash", "dash", "zsh", "ksh", "mksh"];
const SANDBOX_MARKERS: &[&str] = &["bwrap", "bubblewrap", "nixpak", "flatpak", "firejail"];
/// What nixpkgs' `buildFHSEnv` writes into its bwrap launcher: the root file system it
/// binds (`<name>-fhsenv-rootfs`).
const FHS_ENV_MARKER: &str = "-fhsenv-rootfs";

/// Inputs of the locator.
#[derive(Clone, Debug, Default)]
pub struct LocateInput<'a> {
    /// Preference `paths.umu_run`.
    pub preference: Option<&'a Path>,
    /// The host `PATH`.
    pub host_path: Option<&'a OsStr>,
    /// Roots covered by the system binds (`FsLayout::system_roots`).
    pub system_roots: &'a [PathBuf],
    /// The real home directory (deny-list).
    pub home: Option<&'a Path>,
    /// Zaun's configuration, data, cache and state directories (deny-list; game sandboxes
    /// can write below the data directory).
    pub zaun_dirs: &'a [PathBuf],
}

/// Result of locating.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    /// The candidate as found.
    pub path: PathBuf,
    /// Its canonical path.
    pub real_path: PathBuf,
    /// Closure directories, sorted, without entries covered by others.
    pub closure: Vec<PathBuf>,
    /// Wrapper chain, when the candidate is a wrapper.
    pub wrapper: Option<UmuWrapper>,
}

/// Why locating failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LocateError {
    /// No candidate exists (`ZN-HOST-010`).
    #[error("umu-run not found")]
    NotFound,
    /// A closure entry is on the deny-list (`ZN-HOST-012`).
    #[error("{entry} cannot be shown to games: {reason}")]
    ClosureRejected {
        /// The entry.
        entry: PathBuf,
        /// Which rule.
        reason: &'static str,
    },
}

fn under_any(path: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| path.starts_with(r))
}

/// Step 1: the first candidate that exists and is executable.
pub fn find_candidate(fs: &dyn Fs, input: &LocateInput<'_>) -> Option<PathBuf> {
    if let Some(p) = input.preference.filter(|p| p.is_absolute())
        && is_executable(fs, p)
    {
        return Some(p.to_path_buf());
    }
    let fixed = UMU_SEARCH
        .iter()
        .map(PathBuf::from)
        .find(|p| is_executable(fs, p));
    fixed.or_else(|| find_in(fs, &path_dirs(input.host_path), "umu-run"))
}

/// A parsed `#!` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shebang {
    /// The interpreter as written (after resolving `/usr/bin/env NAME`, when possible).
    pub interpreter: PathBuf,
    /// The basename of the interpreter (`python3.13`, `bash`, `launcher`).
    pub name: String,
}

/// Parse the first line of a script. `env` forms are resolved through `host_path`.
pub fn parse_shebang(fs: &dyn Fs, first_line: &str, host_path: Option<&OsStr>) -> Option<Shebang> {
    let rest = first_line.strip_prefix("#!")?.trim();
    let mut words = rest.split_whitespace();
    let mut interp = PathBuf::from(words.next()?);
    if interp.file_name().is_some_and(|n| n == "env") {
        let name = words.find(|w| !w.starts_with('-'))?;
        interp = find_in(fs, &path_dirs(host_path), name).unwrap_or_else(|| PathBuf::from(name));
    }
    let name = interp.file_name()?.to_string_lossy().into_owned();
    Some(Shebang {
        interpreter: interp,
        name,
    })
}

fn is_shell(name: &str) -> bool {
    SHELLS.contains(&name)
}

fn is_python(name: &str) -> bool {
    name.starts_with("python")
}

fn read_script(fs: &dyn Fs, path: &Path) -> Option<String> {
    fs.read_follow(path, SCRIPT_CAP)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Walk up from `start` to the first ancestor satisfying `pred`.
fn walk_up(start: &Path, pred: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    start
        .ancestors()
        .skip(1)
        .take(8)
        .find(|a| *a != Path::new("/") && pred(a))
        .map(Path::to_path_buf)
}

fn is_dir(fs: &dyn Fs, p: &Path) -> bool {
    fs.metadata_follow(p).is_ok_and(|m| m.is_dir())
}

fn standalone_root(fs: &dyn Fs, interp: &Path) -> Option<PathBuf> {
    walk_up(interp, |a| {
        is_dir(fs, &a.join("bin")) && is_dir(fs, &a.join("lib"))
    })
}

fn pyvenv_home(fs: &dyn Fs, venv: &Path) -> Option<PathBuf> {
    let text = read_script(fs, &venv.join("pyvenv.cfg"))?;
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == "home").then(|| PathBuf::from(v.trim()))
    })
}

/// Step 3: installation roots of a Python interpreter outside the system roots.
fn python_roots(fs: &dyn Fs, interp: &Path, roots: &[PathBuf]) -> Vec<PathBuf> {
    let real = fs
        .canonicalize(interp)
        .unwrap_or_else(|_| interp.to_path_buf());
    if under_any(interp, roots) && under_any(&real, roots) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(venv) = walk_up(interp, |a| fs.exists(&a.join("pyvenv.cfg"))) {
        if let Some(home) = pyvenv_home(fs, &venv) {
            let home = fs.canonicalize(&home).unwrap_or(home);
            if !under_any(&home, roots)
                && let Some(base) = standalone_root(fs, &home.join("python"))
            {
                out.push(base);
            }
        }
        out.push(venv);
    } else if !under_any(&real, roots)
        && let Some(root) = standalone_root(fs, &real)
    {
        out.push(root);
    }
    // The resolved interpreter may live in yet another installation (uv-managed Pythons).
    if !under_any(&real, roots)
        && !out.iter().any(|r| real.starts_with(r))
        && let Some(root) = standalone_root(fs, &real)
    {
        out.push(root);
    }
    out
}

/// Absolute paths mentioned in a shell script.
fn script_paths(text: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace() || "\"'=:;()`$|&<>{}[],".contains(c)) {
        if raw.starts_with('/') && raw.len() > 1 && raw.len() < 4096 {
            out.push(PathBuf::from(raw.trim_end_matches('/')));
        }
        if out.len() > 4096 {
            break;
        }
    }
    out
}

fn has_python_or_umu(fs: &dyn Fs, dir: &Path) -> bool {
    if fs.exists(&dir.join("pyvenv.cfg")) || fs.exists(&dir.join("umu/__init__.py")) {
        return true;
    }
    fs.read_dir(&dir.join("bin")).is_ok_and(|entries| {
        entries
            .iter()
            .any(|e| e.name.to_str().is_some_and(|n| n.starts_with("python")))
    })
}

/// The program a shell wrapper hands over to with its last `exec /abs/path …` line.
pub fn exec_target(text: &str) -> Option<PathBuf> {
    text.lines().rev().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("exec ")?;
        let mut words = rest
            .split_whitespace()
            .map(|w| w.trim_matches(|c| c == '"' || c == '\''));
        let mut word = words.next()?;
        while word.starts_with('-') {
            if word == "-a" {
                words.next()?;
            }
            word = words.next()?;
        }
        word.starts_with('/').then(|| PathBuf::from(word))
    })
}

/// Follow a wrapper chain from `real` (a script whose interpreter is not Python).
fn follow_wrapper(fs: &dyn Fs, real: &Path, host_path: Option<&OsStr>) -> UmuWrapper {
    let mut chain = vec![real.to_path_buf()];
    let mut sandboxed = false;
    let mut fhs_env = false;
    let mut reaches_python = false;
    let mut cur = real.to_path_buf();
    for _ in 0..MAX_CHAIN {
        let Some(text) = read_script(fs, &cur) else {
            break;
        };
        let first = text.lines().next().unwrap_or("");
        let shebang = parse_shebang(fs, first, host_path);
        let interp_name = shebang.as_ref().map(|s| s.name.clone()).unwrap_or_default();
        let interp_text = shebang
            .as_ref()
            .map(|s| s.interpreter.to_string_lossy().into_owned())
            .unwrap_or_default();
        if SANDBOX_MARKERS
            .iter()
            .any(|m| text.contains(m) || interp_text.contains(m))
        {
            sandboxed = true;
        }
        if text.contains(FHS_ENV_MARKER) {
            fhs_env = true;
        }
        if is_python(&interp_name) {
            reaches_python = true;
            break;
        }
        let next = if is_shell(&interp_name) {
            exec_target(&text)
        } else {
            // A non-shell launcher (nixpak's `launcher` takes the config and the program as
            // lines of its script): the last absolute path mentioned is the program.
            text.lines()
                .skip(1)
                .map(str::trim)
                .filter(|l| l.starts_with('/'))
                .last()
                .map(PathBuf::from)
        };
        let Some(next) = next else { break };
        let next = fs.canonicalize(&next).unwrap_or(next);
        if chain.contains(&next) {
            break;
        }
        chain.push(next.clone());
        cur = next;
    }
    UmuWrapper {
        chain,
        sandboxed,
        reaches_python,
        fhs_env,
    }
}

const CREDENTIAL_ENTRIES: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".pki",
    ".password-store",
    ".local/share/keyrings",
    ".config/keepassxc",
    ".aws",
    ".azure",
    ".config/gcloud",
    ".kube",
    ".docker",
    ".netrc",
    ".git-credentials",
    ".config/gh",
    ".config/op",
    ".config/Bitwarden",
    ".mozilla",
    ".thunderbird",
    ".config/google-chrome",
    ".config/chromium",
    ".config/BraveSoftware",
    ".config/vivaldi",
    ".config/microsoft-edge",
    ".config/opera",
    ".librewolf",
    ".zen",
    ".var/app",
    ".bashrc",
    ".profile",
    ".zshrc",
    ".config/fish",
    ".config/autostart",
    ".config/systemd",
    ".local/share/applications",
    ".local/bin",
    ".config/environment.d",
];

/// Step 6: the deny-list subset that applies to closure entries (SECURITY_MODEL §7.2).
pub fn check_closure_entry(entry: &Path, input: &LocateInput<'_>) -> Result<(), LocateError> {
    let reject = |reason| {
        Err(LocateError::ClosureRejected {
            entry: entry.to_path_buf(),
            reason,
        })
    };
    let top = [
        "/",
        "/home",
        "/var",
        "/run",
        "/tmp",
        "/mnt",
        "/media",
        "/run/media",
    ];
    if top.iter().any(|t| entry == Path::new(t)) || input.home.is_some_and(|h| entry == h) {
        return reject("top-level directory");
    }
    if let Some(home) = input.home {
        for c in CREDENTIAL_ENTRIES {
            let e = home.join(c);
            if entry.starts_with(&e) || e.starts_with(entry) {
                return reject("private user data");
            }
        }
    }
    for d in input.zaun_dirs {
        if entry.starts_with(d) || d.starts_with(entry) {
            return reject("writable by games");
        }
    }
    Ok(())
}

fn minimise(mut v: Vec<PathBuf>) -> Vec<PathBuf> {
    v.sort();
    v.dedup();
    let all = v.clone();
    v.retain(|p| !all.iter().any(|q| q != p && p.starts_with(q)));
    v
}

/// Steps 1–4 and 6.
pub fn locate(fs: &dyn Fs, input: &LocateInput<'_>) -> Result<Located, LocateError> {
    let path = find_candidate(fs, input).ok_or(LocateError::NotFound)?;
    let real_path = fs.canonicalize(&path).unwrap_or_else(|_| path.clone());
    let roots = input.system_roots;
    let mut closure = Vec::new();
    if !under_any(&real_path, roots)
        && let Some(parent) = real_path.parent()
    {
        closure.push(parent.to_path_buf());
    }
    let mut wrapper = None;
    let text = read_script(fs, &real_path).unwrap_or_default();
    let first = text.lines().next().unwrap_or("");
    if let Some(sb) = parse_shebang(fs, first, input.host_path) {
        if is_python(&sb.name) {
            closure.extend(python_roots(fs, &sb.interpreter, roots));
        } else {
            if is_shell(&sb.name) {
                let mut added = 0;
                for p in script_paths(&text) {
                    if added >= MAX_SHELL_ADDITIONS {
                        break;
                    }
                    if under_any(&p, roots) || !is_dir(fs, &p) || !has_python_or_umu(fs, &p) {
                        continue;
                    }
                    closure.push(p);
                    added += 1;
                }
            }
            wrapper = Some(follow_wrapper(fs, &real_path, input.host_path));
        }
    }
    let closure = minimise(closure);
    for e in &closure {
        check_closure_entry(e, input)?;
    }
    Ok(Located {
        path,
        real_path,
        closure,
        wrapper,
    })
}

/// Parse `umu-launcher version 1.4.4 (3.14.10 (main, …))`.
pub fn parse_umu_version(text: &str) -> Option<ToolVersion> {
    text.lines().find_map(|l| {
        let rest = l.trim().split("umu-launcher version ").nth(1)?;
        rest.split_whitespace().next()?.parse().ok()
    })
}

/// Step 5 and the problems: run `--version` with a clean environment.
pub fn probe_umu(
    fs: &dyn Fs,
    runner: &dyn CommandRunner,
    input: &LocateInput<'_>,
    probe_path: &str,
    problems: &mut Vec<HostProblem>,
) -> Option<UmuFacts> {
    let located = match locate(fs, input) {
        Ok(l) => l,
        Err(LocateError::NotFound) => {
            problems.push(HostProblem::new(
                ErrorCode::HostUmuNotFound,
                format!("umu-run not found in {} or $PATH", UMU_SEARCH.join(", ")),
            ));
            return None;
        }
        Err(e @ LocateError::ClosureRejected { .. }) => {
            problems.push(HostProblem::new(
                ErrorCode::HostUmuClosureRejected,
                e.to_string(),
            ));
            return None;
        }
    };
    let cmd = CommandSpec::new(&located.path, &["--version"])
        .env("PATH", probe_path)
        .timeout(std::time::Duration::from_secs(15));
    let version = runner.run(&cmd).ok().and_then(|o| {
        let mut t = o.stdout_text();
        t.push_str(&o.stderr_text());
        parse_umu_version(&t)
    });
    match version {
        Some(v) if v < super::facts::UMU_MIN_VERSION => problems.push(HostProblem::new(
            ErrorCode::HostUmuTooOld,
            format!(
                "{} is version {v}; at least {} is required",
                located.path.display(),
                super::facts::UMU_MIN_VERSION
            ),
        )),
        None => problems.push(HostProblem {
            code: ErrorCode::HostUmuTooOld,
            detail: format!(
                "could not read the version of {} ($ {})",
                located.path.display(),
                cmd.display()
            ),
            // A sandbox wrapper is the usual reason (UMU_INTEGRATION §2, last paragraph).
            hint: located
                .wrapper
                .as_ref()
                .filter(|w| w.breaks_nesting())
                .map(|_| super::facts::HostHint::UmuWrapper),
        }),
        Some(_) => {}
    }
    Some(UmuFacts {
        path: located.path,
        version,
        closure: located.closure,
        wrapper: located.wrapper,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line() {
        assert_eq!(
            parse_umu_version(
                "umu-launcher version 1.4.4 (3.14.10 (main, Oct 25 2026, 17:12:00) [GCC 16.2.0])\n"
            ),
            Some(ToolVersion::new(1, 4, 4))
        );
        assert_eq!(parse_umu_version("garbage"), None);
    }

    #[test]
    fn exec_targets() {
        assert_eq!(
            exec_target("#!/bin/bash\ncd x\nexec /nix/store/a-env/bin/umu-run \"$@\"\n"),
            Some(PathBuf::from("/nix/store/a-env/bin/umu-run"))
        );
        assert_eq!(
            exec_target("#!/bin/sh\nexec -a \"$0\" \"/nix/store/b/bin/.umu-run-wrapped\" \"$@\""),
            Some(PathBuf::from("/nix/store/b/bin/.umu-run-wrapped"))
        );
        assert_eq!(exec_target("exec \"${cmd[@]}\""), None);
    }
}
