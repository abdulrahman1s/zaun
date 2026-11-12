//! `probe-payload` (SEC-002, TESTING_STRATEGY §8): the program run inside a real sandbox in
//! place of `umu-run`. Each probe attempts something the sandbox must (or may) allow and
//! prints one line:
//!
//! ```text
//! PROBE <id> <ALLOWED|DENIED|ERROR|INFO> <detail>
//! ```
//!
//! Arguments are `key=value` pairs (all optional; a probe whose input is missing is skipped):
//!
//! | Key | Used by |
//! |---|---|
//! | `only=P-01,P-20` | run only these probes (default: all except P-18) |
//! | `home` | P-01…P-04, P-24: the `$HOME` path (a fixture home with decoys on the host) |
//! | `data`, `config` | P-05: EasyPlay's data directory and configuration file |
//! | `other_prefix` | P-06 |
//! | `proton`, `slr` | P-07 |
//! | `tcp=HOST:PORT` | P-09: a listener in the host's network namespace |
//! | `abstract=NAME` | P-10: an abstract Unix socket bound on the host |
//! | `bus` | P-11: the host's session bus socket path |
//! | `sock` (repeatable) | P-12: host sockets that must not be reachable |
//! | `host_pid`, `runner` | P-13: a host PID and a word of its command line |
//! | `kbd`, `block` (repeatable) | P-17: keyboard event nodes and block devices of the host |
//! | `ro`, `rw` | P-23, P-24: granted folders (read-only, read-write) |
//! | `exe` | P-26: the game executable in a read-only game folder |
//! | `daemon=TOKEN` | P-18: leave a double-forked process named TOKEN and exit |

#![allow(
    unsafe_code,
    clippy::disallowed_methods,
    clippy::print_stdout,
    clippy::undocumented_unsafe_blocks
)]

use std::collections::BTreeMap;
use std::ffi::CString;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

type Args = BTreeMap<String, Vec<String>>;

fn emit(id: &str, verdict: &str, detail: impl AsRef<str>) {
    let detail = detail.as_ref().replace('\n', " ");
    println!("PROBE {id} {verdict} {detail}");
    let _ = std::io::stdout().flush();
}

fn verdict(allowed: bool) -> &'static str {
    if allowed { "ALLOWED" } else { "DENIED" }
}

fn one<'a>(a: &'a Args, k: &str) -> Option<&'a str> {
    a.get(k).and_then(|v| v.first()).map(String::as_str)
}

fn all<'a>(a: &'a Args, k: &str) -> &'a [String] {
    a.get(k).map_or(&[], Vec::as_slice)
}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn errname(e: i32) -> String {
    match e {
        0 => "ok".into(),
        libc::EPERM => "EPERM".into(),
        libc::ENOENT => "ENOENT".into(),
        libc::EACCES => "EACCES".into(),
        libc::EROFS => "EROFS".into(),
        libc::ENOSYS => "ENOSYS".into(),
        libc::EINVAL => "EINVAL".into(),
        libc::EFAULT => "EFAULT".into(),
        libc::ENOTTY => "ENOTTY".into(),
        libc::EBADF => "EBADF".into(),
        libc::ESRCH => "ESRCH".into(),
        libc::ENXIO => "ENXIO".into(),
        libc::ECONNREFUSED => "ECONNREFUSED".into(),
        libc::ENETUNREACH => "ENETUNREACH".into(),
        libc::EAFNOSUPPORT => "EAFNOSUPPORT".into(),
        libc::ENODEV => "ENODEV".into(),
        libc::EXDEV => "EXDEV".into(),
        e => format!("errno{e}"),
    }
}

fn io_name(e: &std::io::Error) -> String {
    e.raw_os_error()
        .map_or_else(|| e.kind().to_string(), errname)
}

fn read_path(p: &Path) -> Result<String, String> {
    let mut s = String::new();
    std::fs::File::open(p)
        .and_then(|mut f| f.read_to_string(&mut s))
        .map(|_| s)
        .map_err(|e| io_name(&e))
}

fn write_path(p: &Path, text: &str) -> Result<(), String> {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    std::fs::write(p, text).map_err(|e| io_name(&e))
}

/// Try to read each path; ALLOWED when any read succeeds.
fn reads(id: &str, paths: &[PathBuf]) {
    let mut any = false;
    let mut d = Vec::new();
    for p in paths {
        let r = read_path(p);
        any |= r.is_ok();
        d.push(format!(
            "{}={}",
            p.display(),
            r.err().unwrap_or_else(|| "read".into())
        ));
    }
    emit(id, verdict(any), d.join(" "));
}

fn p01_to_p04(a: &Args) {
    let Some(home) = one(a, "home").map(PathBuf::from) else {
        return;
    };
    reads("P-01", &[home.join(".ssh/id_ed25519")]);
    let listing: Vec<String> = std::fs::read_dir(&home)
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    let secret = read_path(&home.join("Documents/secret.txt"));
    emit(
        "P-02",
        verdict(secret.is_ok() || listing.iter().any(|n| n == "Documents")),
        format!(
            "secret={} home=[{}]",
            secret.err().unwrap_or_else(|| "read".into()),
            listing.join(",")
        ),
    );
    reads(
        "P-03",
        &[
            home.join(".mozilla/firefox/profiles.ini"),
            home.join(".config/google-chrome/Default/Cookies"),
        ],
    );
    // Writes land in the private home (allowed); the harness checks the host side.
    let mut d = Vec::new();
    for rel in [".bashrc", ".config/autostart/x.desktop", ".local/bin/x"] {
        let r = write_path(&home.join(rel), "probe-payload was here\n");
        d.push(format!(
            "{rel}={}",
            r.err().unwrap_or_else(|| "written".into())
        ));
    }
    emit("P-04", "INFO", d.join(" "));
}

fn p05_to_p07(a: &Args) {
    if let (Some(data), Some(cfg)) = (one(a, "data"), one(a, "config")) {
        let data = PathBuf::from(data);
        let manifests = std::fs::read_dir(data.join("runtime-manifests")).is_ok();
        let mut d = vec![format!(
            "runtime-manifests={}",
            if manifests { "listed" } else { "absent" }
        )];
        let mut any = manifests;
        for p in [data.join("library.db"), PathBuf::from(cfg)] {
            let r = read_path(&p);
            any |= r.is_ok();
            d.push(format!(
                "{}={}",
                p.display(),
                r.err().unwrap_or_else(|| "read".into())
            ));
        }
        emit("P-05", verdict(any), d.join(" "));
    }
    if let Some(other) = one(a, "other_prefix").map(PathBuf::from) {
        let r = read_path(&other.join("marker.txt"));
        // No directory creation: a write into the sandbox's own tmpfs proves nothing.
        let w = std::fs::write(other.join("probe.txt"), "x").map_err(|e| io_name(&e));
        emit(
            "P-06",
            verdict(r.is_ok() || w.is_ok()),
            format!("read={:?} write={:?}", r.err(), w.err()),
        );
    }
    let mut d = Vec::new();
    let mut any = false;
    for (k, existing) in [("proton", "proton"), ("slr", "toolmanifest.vdf")] {
        if let Some(dir) = one(a, k).map(PathBuf::from) {
            let w = std::fs::write(dir.join("probe.txt"), "x").map_err(|e| io_name(&e));
            let m = std::fs::OpenOptions::new()
                .append(true)
                .open(dir.join(existing))
                .map_err(|e| io_name(&e));
            // New top-level entries of an Isolated runtime land in the sandbox's synthesized
            // (tmpfs) top level; the harness checks the host copy. Existing files must not
            // be writable.
            any |= m.is_ok() || (k == "proton" && w.is_ok());
            d.push(format!(
                "{k}_create={} {k}_modify={}",
                w.err().unwrap_or_else(|| "written".into()),
                m.err().unwrap_or_else(|| "opened".into())
            ));
        }
    }
    if !d.is_empty() {
        emit("P-07", verdict(any), d.join(" "));
    }
}

fn p09_to_p12(a: &Args) {
    if let Some(addr) = one(a, "tcp") {
        let r = addr
            .to_socket_addrs()
            .map_err(|e| io_name(&e))
            .and_then(|mut it| it.next().ok_or_else(|| "no address".to_owned()))
            .and_then(|sa| {
                TcpStream::connect_timeout(&sa, Duration::from_secs(2)).map_err(|e| io_name(&e))
            });
        emit(
            "P-09",
            verdict(r.is_ok()),
            r.err().unwrap_or_else(|| "connected".into()),
        );
    }
    if let Some(name) = one(a, "abstract") {
        let r = SocketAddr::from_abstract_name(name.as_bytes())
            .and_then(|sa| UnixStream::connect_addr(&sa))
            .map_err(|e| io_name(&e));
        emit(
            "P-10",
            verdict(r.is_ok()),
            r.err().unwrap_or_else(|| "connected".into()),
        );
    }
    if let Some(bus) = one(a, "bus") {
        let env = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
        let r = UnixStream::connect(bus).map_err(|e| io_name(&e));
        emit(
            "P-11",
            verdict(env.is_some() || r.is_ok()),
            format!(
                "env={} connect={}",
                if env.is_some() { "set" } else { "unset" },
                r.err().unwrap_or_else(|| "connected".into())
            ),
        );
    }
    let socks = all(a, "sock");
    if !socks.is_empty() {
        let mut any = false;
        let mut d = Vec::new();
        for s in socks {
            let r = UnixStream::connect(s).map_err(|e| io_name(&e));
            any |= r.is_ok();
            d.push(format!(
                "{s}={}",
                r.err().unwrap_or_else(|| "connected".into())
            ));
        }
        emit("P-12", verdict(any), d.join(" "));
    }
}

fn p13_to_p17(a: &Args) {
    let pids: Vec<String> = std::fs::read_dir("/proc")
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.bytes().all(|b| b.is_ascii_digit()))
                .collect()
        })
        .unwrap_or_default();
    let runner = one(a, "runner");
    // argv[0] only: the init shim and this program carry `runner=…` in their arguments.
    let seen_runner = runner.is_some_and(|w| {
        pids.iter().any(|p| {
            std::fs::read(format!("/proc/{p}/cmdline")).is_ok_and(|c| {
                let argv0 = c.split(|b| *b == 0).next().unwrap_or_default();
                String::from_utf8_lossy(argv0).contains(w)
            })
        })
    });
    let kill = one(a, "host_pid")
        .and_then(|p| p.parse::<i32>().ok())
        .map(|pid| {
            let r = unsafe { libc::kill(pid, 0) };
            if r == 0 { 0 } else { errno() }
        });
    emit(
        "P-13",
        verdict(seen_runner || kill == Some(0) || kill == Some(libc::EPERM)),
        format!(
            "pids={} runner_visible={seen_runner} kill={}",
            pids.len(),
            kill.map_or_else(|| "-".into(), errname)
        ),
    );

    let shadow = read_path(Path::new("/etc/shadow"));
    let passwd = read_path(Path::new("/etc/passwd")).unwrap_or_default();
    let machine_id = read_path(Path::new("/etc/machine-id")).unwrap_or_default();
    let hostname_file = read_path(Path::new("/etc/hostname"));
    let lines = passwd.lines().filter(|l| !l.is_empty()).count();
    emit(
        "P-14",
        verdict(shadow.is_ok() || lines > 2),
        format!(
            "shadow={} passwd_lines={lines} etc_hostname={} machine_id={}",
            shadow.err().unwrap_or_else(|| "read".into()),
            hostname_file.err().unwrap_or_else(|| "read".into()),
            machine_id.trim()
        ),
    );
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    let host = if unsafe { libc::uname(&mut uts) } == 0 {
        let raw: Vec<u8> = uts
            .nodename
            .iter()
            .take_while(|c| **c != 0)
            .map(|c| *c as u8)
            .collect();
        String::from_utf8_lossy(&raw).into_owned()
    } else {
        String::new()
    };
    emit("P-15", "INFO", format!("hostname={host}"));
    let aws = std::env::var_os("AWS_SECRET_ACCESS_KEY");
    emit(
        "P-16",
        verdict(aws.is_some()),
        if aws.is_some() { "present" } else { "absent" },
    );

    let mut any = false;
    let mut d = Vec::new();
    let mut nodes = vec![("/dev/uinput".to_owned(), libc::O_RDWR)];
    nodes.extend(all(a, "kbd").iter().map(|k| (k.clone(), libc::O_RDONLY)));
    nodes.extend(all(a, "block").iter().map(|k| (k.clone(), libc::O_RDONLY)));
    for (n, flags) in nodes {
        let Ok(c) = CString::new(n.as_str()) else {
            continue;
        };
        let fd = unsafe { libc::open(c.as_ptr(), flags | libc::O_CLOEXEC | libc::O_NONBLOCK) };
        if fd >= 0 {
            any = true;
            unsafe { libc::close(fd) };
            d.push(format!("{n}=opened"));
        } else {
            d.push(format!("{n}={}", errname(errno())));
        }
    }
    let events: Vec<String> = std::fs::read_dir("/dev/input")
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    d.push(format!("dev_input=[{}]", events.join(",")));
    emit("P-17", verdict(any), d.join(" "));
}

fn p19() {
    let mut macs = Vec::new();
    if let Ok(r) = std::fs::read_dir("/sys/class/net") {
        for e in r.filter_map(Result::ok) {
            if let Ok(m) = read_path(&e.path().join("address")) {
                let m = m.trim().to_owned();
                if !m.is_empty() && m != "00:00:00:00:00:00" {
                    macs.push(format!("{}", e.file_name().to_string_lossy()));
                }
            }
        }
    }
    emit(
        "P-19",
        verdict(!macs.is_empty()),
        format!("interfaces=[{}]", macs.join(",")),
    );
}

/// `syscall(2)` returning 0 or the errno.
fn sys(nr: libc::c_long, a: [libc::c_long; 5]) -> (libc::c_long, i32) {
    let r = unsafe { libc::syscall(nr, a[0], a[1], a[2], a[3], a[4]) };
    if r < 0 { (r, errno()) } else { (r, 0) }
}

/// One system call through the `i386` entry (`int 0x80`) of a 64-bit process.
#[cfg(target_arch = "x86_64")]
fn sys32(nr: u32, a: [u32; 3]) -> i32 {
    let mut ret: i64 = i64::from(nr);
    unsafe {
        // rbx is reserved by LLVM: swap the first argument in and out around the call.
        std::arch::asm!(
            "xchg rbx, {b}",
            "int 0x80",
            "xchg rbx, {b}",
            b = inout(reg) u64::from(a[0]) => _,
            inout("rax") ret,
            in("rcx") u64::from(a[1]),
            in("rdx") u64::from(a[2]),
            lateout("r8") _, lateout("r9") _, lateout("r10") _, lateout("r11") _,
        );
    }
    let ret = ret as i32;
    if ret < 0 { -ret } else { 0 }
}

fn p20_p21() {
    let mut d = Vec::new();
    let mut any = false;
    let mut note = |name: &str, (r, e): (libc::c_long, i32)| {
        if e != libc::EPERM {
            any = true;
        }
        if e == 0 && r >= 0 && name == "userfaultfd" {
            unsafe { libc::close(r as i32) };
        }
        d.push(format!("{name}={}", errname(e)));
    };
    // KEYCTL_GET_KEYRING_ID of the session keyring: succeeds without the filter.
    note("keyctl", sys(libc::SYS_keyctl, [0, -3, 0, 0, 0]));
    let ty = c"easyplay-none";
    let desc = c"x";
    note(
        "add_key",
        sys(
            libc::SYS_add_key,
            [ty.as_ptr() as _, desc.as_ptr() as _, 0, 0, -3],
        ),
    );
    note(
        "perf_event_open",
        sys(libc::SYS_perf_event_open, [0, 0, -1, -1, 0]),
    );
    note("bpf", sys(libc::SYS_bpf, [5, 0, 0, 0, 0]));
    note(
        "io_uring_setup",
        sys(libc::SYS_io_uring_setup, [1, 0, 0, 0, 0]),
    );
    // UFFD_USER_MODE_ONLY: allowed for unprivileged users without the filter.
    note("userfaultfd", sys(libc::SYS_userfaultfd, [1, 0, 0, 0, 0]));
    #[cfg(target_arch = "x86_64")]
    {
        let e = sys32(288, [0, (-3i32) as u32, 0]);
        note("keyctl_i386", (if e == 0 { 0 } else { -1 }, e));
        // x32 `getpid`: refused with ENOSYS by the filter (and on kernels without x32).
        let (_, e) = sys(0x4000_0000 | 39, [0; 5]);
        d.push(format!("getpid_x32={}", errname(e)));
    }
    emit("P-20", verdict(any), d.join(" "));

    let mut d = Vec::new();
    let mut any = false;
    let c: u8 = b'x';
    for fd in 0..3 {
        let r = unsafe { libc::ioctl(fd, libc::TIOCSTI, &c) };
        let e = if r == 0 { 0 } else { errno() };
        any |= r == 0;
        d.push(format!("fd{fd}={}", errname(e)));
    }
    let tty = unsafe { libc::open(c"/dev/tty".as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
    if tty >= 0 {
        let r = unsafe { libc::ioctl(tty, libc::TIOCSTI, &c) };
        any |= r == 0;
        d.push(format!(
            "tty={}",
            if r == 0 {
                "ok".into()
            } else {
                errname(errno())
            }
        ));
        unsafe { libc::close(tty) };
    } else {
        d.push(format!("tty_open={}", errname(errno())));
    }
    emit("P-21", verdict(any), d.join(" "));
}

/// A nested user and mount namespace with a tmpfs: what pressure-vessel's bwrap does.
fn p22() {
    let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        let code = (|| {
            if unsafe { libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) } != 0 {
                return 10;
            }
            let _ = std::fs::write("/proc/self/setgroups", "deny");
            if std::fs::write("/proc/self/uid_map", format!("0 {uid} 1")).is_err()
                || std::fs::write("/proc/self/gid_map", format!("0 {gid} 1")).is_err()
            {
                return 11;
            }
            let r = unsafe {
                libc::mount(
                    c"none".as_ptr(),
                    c"/tmp".as_ptr(),
                    c"tmpfs".as_ptr(),
                    0,
                    std::ptr::null(),
                )
            };
            if r != 0 {
                return 12;
            }
            0
        })();
        unsafe { libc::_exit(code) };
    }
    let mut status = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };
    let code = if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        -1
    };
    let step = match code {
        0 => "mounted",
        10 => "unshare failed",
        11 => "id map failed",
        12 => "mount failed",
        _ => "child died",
    };
    emit("P-22", verdict(code == 0), step);
}

fn p23_p24_p26(a: &Args) {
    if let Some(ro) = one(a, "ro").map(PathBuf::from) {
        let r = read_path(&ro.join("granted.txt"));
        emit(
            "P-23R",
            verdict(r.is_ok()),
            r.err().unwrap_or_else(|| "read".into()),
        );
        let w = std::fs::write(ro.join("probe.txt"), "x").map_err(|e| io_name(&e));
        emit(
            "P-23W",
            verdict(w.is_ok()),
            w.err().unwrap_or_else(|| "written".into()),
        );
    }
    if let (Some(rw), Some(home)) = (one(a, "rw").map(PathBuf::from), one(a, "home")) {
        let w = std::fs::write(rw.join("probe.txt"), "x").map_err(|e| io_name(&e));
        emit(
            "P-24W",
            verdict(w.is_ok()),
            w.err().unwrap_or_else(|| "written".into()),
        );
        let link = rw.join("ssh-link");
        let _ = std::fs::remove_file(&link);
        let made = std::os::unix::fs::symlink(Path::new(home).join(".ssh"), &link)
            .map_err(|e| io_name(&e));
        let r = read_path(&link.join("id_ed25519"));
        emit(
            "P-24L",
            verdict(r.is_ok()),
            format!(
                "symlink={} read={}",
                made.err().unwrap_or_else(|| "made".into()),
                r.err().unwrap_or_else(|| "read".into())
            ),
        );
    }
    if let Some(exe) = one(a, "exe").map(PathBuf::from) {
        let w = std::fs::OpenOptions::new()
            .write(true)
            .open(&exe)
            .map_err(|e| io_name(&e));
        let tmp = exe.with_extension("new");
        let rn = std::fs::write(&tmp, "MZ")
            .and_then(|()| std::fs::rename(&tmp, &exe))
            .map_err(|e| io_name(&e));
        emit(
            "P-26",
            verdict(w.is_ok() || rn.is_ok()),
            format!("open_write={:?} replace={:?}", w.err(), rn.err()),
        );
    }
}

fn p18(token: &str) {
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        if unsafe { libc::fork() } != 0 {
            unsafe { libc::_exit(0) };
        }
        unsafe { libc::setsid() };
        if let Ok(name) = CString::new(token) {
            unsafe { libc::prctl(libc::PR_SET_NAME, name.as_ptr()) };
        }
        for fd in 0..3 {
            unsafe { libc::close(fd) };
        }
        std::thread::sleep(Duration::from_secs(600));
        unsafe { libc::_exit(0) };
    }
    let mut status = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };
    emit("P-18", "INFO", "daemon started");
}

fn main() {
    let mut a: Args = BTreeMap::new();
    for arg in std::env::args().skip(1) {
        if let Some((k, v)) = arg.split_once('=') {
            a.entry(k.to_owned()).or_default().push(v.to_owned());
        }
    }
    let only: Option<Vec<String>> =
        one(&a, "only").map(|o| o.split(',').map(str::to_owned).collect());
    let want = |id: &str| only.as_ref().is_none_or(|o| o.iter().any(|x| x == id));

    let seccomp = read_path(Path::new("/proc/self/status"))
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("Seccomp:").map(|v| v.trim().to_owned()))
        .unwrap_or_default();
    emit(
        "P-00",
        "INFO",
        format!("uid={} seccomp={seccomp}", unsafe { libc::getuid() }),
    );

    if let Some(token) = one(&a, "daemon") {
        p18(token);
        return;
    }
    // Fork-based probes first, while the process has one thread.
    if want("P-22") {
        p22();
    }
    if want("P-01") {
        p01_to_p04(&a);
    }
    if want("P-05") {
        p05_to_p07(&a);
    }
    if want("P-09") {
        p09_to_p12(&a);
    }
    if want("P-13") {
        p13_to_p17(&a);
    }
    if want("P-19") {
        p19();
    }
    if want("P-20") {
        p20_p21();
    }
    if want("P-23") {
        p23_p24_p26(&a);
    }
    emit("P-99", "INFO", "done");
}
