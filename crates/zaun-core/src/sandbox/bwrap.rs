//! `render(plan)`: plan → `bwrap` options (passed NUL-separated through `--args`), descriptor
//! payloads and the in-sandbox command (BUBBLEWRAP_SANDBOX §3, §7).
//!
//! `--perms` and `--size` immediately precede the option they modify. Arguments are never
//! joined into a shell string; [`display_lines`] exists only for golden tests and the
//! inspector.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use super::plan::{FD_ARGS, FdPayload, FileDataOp, MountKind, MountMode, SandboxPlan};

/// Rendering failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// An argument contains a NUL byte (cannot be passed through `--args`).
    #[error("argument {index} contains a NUL byte")]
    Nul {
        /// Index in the option list.
        index: usize,
    },
    /// A `FileData` mount references a descriptor without payload.
    #[error("descriptor {fd} has no payload")]
    MissingPayload {
        /// Descriptor.
        fd: i32,
    },
}

/// The rendered sandbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedSandbox {
    /// Options grouped per logical option (`["--perms", "1777", "--tmpfs", "/tmp"]` is one
    /// group).
    pub groups: Vec<Vec<OsString>>,
    /// Every option, flattened (the `--args` payload content).
    pub args: Vec<OsString>,
    /// Descriptor payloads.
    pub fds: Vec<FdPayload>,
    /// In-sandbox command (after `--`).
    pub command: Vec<OsString>,
}

impl RenderedSandbox {
    /// The `--args` payload: each argument followed by one NUL byte.
    pub fn args_payload(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for a in &self.args {
            out.extend_from_slice(a.as_bytes());
            out.push(0);
        }
        out
    }

    /// The real command line: `bwrap --args 3 -- <command…>`.
    pub fn argv(&self, bwrap: &Path) -> Vec<OsString> {
        let mut v: Vec<OsString> = vec![
            bwrap.as_os_str().to_owned(),
            "--args".into(),
            FD_ARGS.to_string().into(),
            "--".into(),
        ];
        v.extend(self.command.iter().cloned());
        v
    }
}

fn s(x: impl AsRef<OsStr>) -> OsString {
    x.as_ref().to_owned()
}

fn perms(p: u32) -> OsString {
    OsString::from(format!("{p:04o}"))
}

/// Render the option groups of one mount.
pub fn mount_group(kind: &MountKind) -> Vec<OsString> {
    match kind {
        MountKind::Bind {
            src,
            dst,
            mode,
            try_,
        } => {
            let opt = match (mode, try_) {
                (MountMode::Ro, false) => "--ro-bind",
                (MountMode::Ro, true) => "--ro-bind-try",
                (MountMode::Rw, false) => "--bind",
                (MountMode::Rw, true) => "--bind-try",
            };
            vec![s(opt), s(src), s(dst)]
        }
        MountKind::DevBind { src, dst, try_ } => {
            let opt = if *try_ {
                "--dev-bind-try"
            } else {
                "--dev-bind"
            };
            vec![s(opt), s(src), s(dst)]
        }
        MountKind::Tmpfs {
            dst,
            size,
            perms: p,
        } => {
            let mut v = Vec::new();
            if let Some(p) = p {
                v.extend([s("--perms"), perms(*p)]);
            }
            if let Some(size) = size {
                v.extend([s("--size"), s(size.to_string())]);
            }
            v.extend([s("--tmpfs"), s(dst)]);
            v
        }
        MountKind::Proc { dst } => vec![s("--proc"), s(dst)],
        MountKind::Dev { dst } => vec![s("--dev"), s(dst)],
        MountKind::Dir { dst, perms: p } => {
            let mut v = Vec::new();
            if let Some(p) = p {
                v.extend([s("--perms"), perms(*p)]);
            }
            v.extend([s("--dir"), s(dst)]);
            v
        }
        MountKind::Symlink { target, dst } => vec![s("--symlink"), s(target), s(dst)],
        MountKind::FileData {
            dst,
            fd,
            perms: p,
            op,
        } => {
            let opt = match op {
                FileDataOp::RoBindData => "--ro-bind-data",
                FileDataOp::File => "--file",
            };
            vec![s("--perms"), perms(*p), s(opt), s(fd.to_string()), s(dst)]
        }
        MountKind::OverlayTmp { lower, dst } => {
            vec![s("--overlay-src"), s(lower), s("--tmp-overlay"), s(dst)]
        }
        MountKind::OverlayRw {
            lower,
            upper,
            work,
            dst,
        } => vec![
            s("--overlay-src"),
            s(lower),
            s("--overlay"),
            s(upper),
            s(work),
            s(dst),
        ],
    }
}

/// Render a plan.
pub fn render(plan: &SandboxPlan) -> Result<RenderedSandbox, RenderError> {
    let mut groups: Vec<Vec<OsString>> = Vec::new();
    let ns = &plan.namespaces;
    let flag = |g: &mut Vec<Vec<OsString>>, on: bool, opt: &str| {
        if on {
            g.push(vec![s(opt)]);
        }
    };
    flag(&mut groups, ns.user, "--unshare-user");
    flag(&mut groups, ns.ipc, "--unshare-ipc");
    flag(&mut groups, ns.pid, "--unshare-pid");
    flag(&mut groups, ns.uts, "--unshare-uts");
    flag(&mut groups, ns.cgroup_try, "--unshare-cgroup-try");
    flag(&mut groups, ns.net, "--unshare-net");
    if let Some(h) = &plan.hostname {
        groups.push(vec![s("--hostname"), s(h)]);
    }
    flag(&mut groups, ns.die_with_parent, "--die-with-parent");
    flag(&mut groups, ns.new_session, "--new-session");
    if let Some(fd) = ns.json_status_fd {
        groups.push(vec![s("--json-status-fd"), s(fd.to_string())]);
    }
    if let Some(fd) = ns.sync_fd {
        groups.push(vec![s("--sync-fd"), s(fd.to_string())]);
    }
    if let Some(fd) = ns.seccomp_fd {
        groups.push(vec![s("--seccomp"), s(fd.to_string())]);
    }
    for m in &plan.mounts {
        if let MountKind::FileData { fd, .. } = &m.kind
            && !plan.fds.iter().any(|p| p.fd == *fd)
        {
            return Err(RenderError::MissingPayload { fd: *fd });
        }
        groups.push(mount_group(&m.kind));
    }
    if plan.env.clear {
        groups.push(vec![s("--clearenv")]);
    }
    for name in &plan.env.unset {
        groups.push(vec![s("--unsetenv"), s(name)]);
    }
    for (name, e) in &plan.env.set {
        groups.push(vec![s("--setenv"), s(name), s(&e.value)]);
    }
    groups.push(vec![s("--chdir"), s(&plan.chdir)]);
    let args: Vec<OsString> = groups.iter().flatten().cloned().collect();
    for (index, a) in args.iter().chain(plan.command.iter()).enumerate() {
        if a.as_bytes().contains(&0) {
            return Err(RenderError::Nul { index });
        }
    }
    Ok(RenderedSandbox {
        groups,
        args,
        fds: plan.fds.clone(),
        command: plan.command.clone(),
    })
}

fn quote(a: &OsStr) -> String {
    let t = a.to_string_lossy();
    if t.is_empty() || t.contains(' ') {
        format!("\"{t}\"")
    } else {
        t.into_owned()
    }
}

/// One line per option group, arguments containing spaces in double quotes (the format of
/// BUBBLEWRAP_SANDBOX §6). For display and golden tests only.
pub fn display_lines(r: &RenderedSandbox) -> String {
    let mut out = String::new();
    for g in &r.groups {
        let line: Vec<String> = g.iter().map(|a| quote(a)).collect();
        out.push_str(&line.join(" "));
        out.push('\n');
    }
    out
}

/// The command as one display line (`-- init … <args>`).
pub fn display_command(r: &RenderedSandbox) -> String {
    let mut parts = vec!["--".to_owned()];
    parts.extend(r.command.iter().map(|a| quote(a)));
    parts.join(" ")
}
