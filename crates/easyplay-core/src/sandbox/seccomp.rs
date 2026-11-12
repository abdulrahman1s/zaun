//! The seccomp deny-list program (BUBBLEWRAP_SANDBOX §9, SEC-001).
//!
//! One classic-BPF program covers the system-call ABIs of an x86-64 kernel:
//!
//! * `x86_64` and `i386` (32-bit Wine processes) get the same deny-list, each with its own
//!   system-call numbers ([`RULES`]);
//! * the `x32` ABI (an `x86_64` audit architecture with bit 30 set in the number) is refused
//!   as a whole with `ENOSYS`, as if the kernel had been built without it (the default of
//!   most distributions). Nothing in Wine, Proton or the Steam Linux Runtime uses it, and
//!   refusing it closes the alias of every `x86_64` rule;
//! * any other architecture value is allowed (the default action of a deny-list).
//!
//! The rule blocks of each ABI are compiled with `seccompiler` (which targets one
//! architecture and kills on a mismatch) and composed here under a dispatch on
//! `seccomp_data.arch`. The planner never builds the program: it reserves descriptor 7 and
//! the spawner asks [`program`] for the cached bytes.

use crate::error::Error;

/// Whether this build can produce the program (feature `seccomp` on `x86_64`).
pub const AVAILABLE: bool = cfg!(all(feature = "seccomp", target_arch = "x86_64"));

/// The variable part of the filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Profile {
    /// Refuse `AF_PACKET` sockets (`network = None`: the sandbox owns its loopback-only
    /// network namespace and would otherwise hold `CAP_NET_RAW` there).
    pub deny_packet_sockets: bool,
}

impl Profile {
    /// The profile matching a rendered `bwrap` argument list: packet sockets are refused
    /// when the network namespace is unshared.
    pub fn for_args<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> Self {
        Self {
            deny_packet_sockets: args.iter().any(|a| a.as_ref() == "--unshare-net"),
        }
    }
}

/// System-call numbers of one entry of the deny-list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Syscall {
    /// Name.
    pub name: &'static str,
    /// `x86_64` number; `None` when the call does not exist in that ABI.
    pub x86_64: Option<u32>,
    /// `i386` number; `None` when the call does not exist in that ABI.
    pub i386: Option<u32>,
}

const fn sc(name: &'static str, x86_64: Option<u32>, i386: Option<u32>) -> Syscall {
    Syscall { name, x86_64, i386 }
}

/// System calls refused unconditionally with `EPERM` (BUBBLEWRAP_SANDBOX §9, rows 1–3).
/// `i386` also has the legacy `stime` and the 64-bit-time variants of the clock setters,
/// which are listed so the 32-bit ABI cannot bypass the `x86_64` rows.
pub const RULES: &[Syscall] = &[
    // Kernel keyrings.
    sc("add_key", Some(248), Some(286)),
    sc("request_key", Some(249), Some(287)),
    sc("keyctl", Some(250), Some(288)),
    // Exploit-prone surfaces.
    sc("bpf", Some(321), Some(357)),
    sc("perf_event_open", Some(298), Some(336)),
    sc("userfaultfd", Some(323), Some(374)),
    sc("io_uring_setup", Some(425), Some(425)),
    sc("io_uring_enter", Some(426), Some(426)),
    sc("io_uring_register", Some(427), Some(427)),
    // Privileged or legacy.
    sc("kexec_load", Some(246), Some(283)),
    sc("kexec_file_load", Some(320), None),
    sc("reboot", Some(169), Some(88)),
    sc("swapon", Some(167), Some(87)),
    sc("swapoff", Some(168), Some(115)),
    sc("init_module", Some(175), Some(128)),
    sc("finit_module", Some(313), Some(350)),
    sc("delete_module", Some(176), Some(129)),
    sc("acct", Some(163), Some(51)),
    sc("quotactl", Some(179), Some(131)),
    sc("quotactl_fd", Some(443), Some(443)),
    sc("settimeofday", Some(164), Some(79)),
    sc("stime", None, Some(25)),
    sc("clock_settime", Some(227), Some(264)),
    sc("clock_settime64", None, Some(404)),
    sc("clock_adjtime", Some(305), Some(343)),
    sc("clock_adjtime64", None, Some(405)),
    sc("adjtimex", Some(159), Some(124)),
    sc("syslog", Some(103), Some(103)),
    sc("uselib", Some(134), Some(86)),
    sc("nfsservctl", Some(180), Some(169)),
    sc("vhangup", Some(153), Some(111)),
    sc("lookup_dcookie", Some(212), Some(253)),
    sc("open_by_handle_at", Some(304), Some(342)),
    sc("name_to_handle_at", Some(303), Some(341)),
];

/// `ioctl`: refused with `EPERM` for the requests in [`IOCTL_DENIED`].
pub const IOCTL: Syscall = sc("ioctl", Some(16), Some(54));
/// `socket`: refused with `EAFNOSUPPORT` for the families in [`SOCKET_FAMILIES_DENIED`].
pub const SOCKET: Syscall = sc("socket", Some(41), Some(359));
/// `personality`: refused with `EPERM` unless the persona is in [`PERSONALITY_ALLOWED`].
pub const PERSONALITY: Syscall = sc("personality", Some(135), Some(136));

/// `TIOCSTI`, `TIOCLINUX` (terminal input injection). Compared on the low 32 bits, as the
/// kernel does (`unsigned int cmd`), so high bits cannot smuggle a request past the filter.
pub const IOCTL_DENIED: &[u32] = &[0x5412, 0x541C];

/// `AF_NETLINK`.
pub const AF_NETLINK: u32 = 16;
/// `NETLINK_AUDIT`, refused only with `AF_NETLINK`.
pub const NETLINK_AUDIT: u32 = 9;
/// `AF_PACKET`, refused when [`Profile::deny_packet_sockets`].
pub const AF_PACKET: u32 = 17;
/// Families refused in every profile: `AF_VSOCK`, `AF_ALG`, `AF_RDS`, `AF_TIPC`, `AF_CAN`,
/// `AF_BLUETOOTH`.
pub const SOCKET_FAMILIES_DENIED: &[u32] = &[40, 38, 21, 30, 29, 31];

/// Personas `personality` may set: `PER_LINUX`, `PER_LINUX32`, both with
/// `ADDR_NO_RANDOMIZE`, and `0xffffffff` (the read-only query Wine and glibc use).
pub const PERSONALITY_ALLOWED: &[u32] = &[0, 0x0008, 0x0004_0000, 0x0004_0008, 0xffff_ffff];

/// `EPERM`.
pub const EPERM: u32 = 1;
/// `ENOSYS`.
pub const ENOSYS: u32 = 38;
/// `EAFNOSUPPORT`.
pub const EAFNOSUPPORT: u32 = 97;

/// `AUDIT_ARCH_X86_64`.
pub const AUDIT_ARCH_X86_64: u32 = 62 | 0x8000_0000 | 0x4000_0000;
/// `AUDIT_ARCH_I386`.
pub const AUDIT_ARCH_I386: u32 = 3 | 0x4000_0000;
/// `__X32_SYSCALL_BIT`.
pub const X32_SYSCALL_BIT: u32 = 0x4000_0000;

/// `SECCOMP_RET_ALLOW`.
pub const RET_ALLOW: u32 = 0x7fff_0000;
/// `SECCOMP_RET_ERRNO`.
pub const RET_ERRNO: u32 = 0x0005_0000;

/// One classic-BPF instruction (`struct sock_filter`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Insn {
    /// Opcode.
    pub code: u16,
    /// Jump offset if true.
    pub jt: u8,
    /// Jump offset if false.
    pub jf: u8,
    /// Operand.
    pub k: u32,
}

/// `BPF_LD | BPF_W | BPF_ABS`.
pub const LD_W_ABS: u16 = 0x20;
/// `BPF_JMP | BPF_JA`.
pub const JMP_JA: u16 = 0x05;
/// `BPF_JMP | BPF_JEQ | BPF_K`.
pub const JMP_JEQ_K: u16 = 0x15;
/// `BPF_JMP | BPF_JGT | BPF_K`.
pub const JMP_JGT_K: u16 = 0x25;
/// `BPF_JMP | BPF_JGE | BPF_K`.
pub const JMP_JGE_K: u16 = 0x35;
/// `BPF_RET | BPF_K`.
pub const RET_K: u16 = 0x06;
/// `BPF_ALU | BPF_AND | BPF_K`.
pub const ALU_AND_K: u16 = 0x54;

/// Offset of `seccomp_data.nr`.
pub const DATA_NR: u32 = 0;
/// Offset of `seccomp_data.arch`.
pub const DATA_ARCH: u32 = 4;
/// Offset of `seccomp_data.args[0]`.
pub const DATA_ARGS: u32 = 16;

/// The program as raw `struct sock_filter` bytes (little endian, 8 bytes per instruction),
/// the format `bwrap --seccomp` reads.
pub fn to_bytes(prog: &[Insn]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prog.len() * 8);
    for i in prog {
        out.extend_from_slice(&i.code.to_le_bytes());
        out.push(i.jt);
        out.push(i.jf);
        out.extend_from_slice(&i.k.to_le_bytes());
    }
    out
}

/// Parse raw `struct sock_filter` bytes.
pub fn from_bytes(bytes: &[u8]) -> Option<Vec<Insn>> {
    if !bytes.len().is_multiple_of(8) {
        return None;
    }
    Some(
        bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|c| Insn {
                code: u16::from_le_bytes([c[0], c[1]]),
                jt: c[2],
                jf: c[3],
                k: u32::from_le_bytes([c[4], c[5], c[6], c[7]]),
            })
            .collect(),
    )
}

/// The input of one filter run (`struct seccomp_data` without the instruction pointer).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SeccompData {
    /// System-call number.
    pub nr: u32,
    /// Audit architecture.
    pub arch: u32,
    /// Arguments.
    pub args: [u64; 6],
}

/// Run `prog` on `data` like the kernel does; `None` for a malformed program (unknown
/// opcode, out-of-range jump or load, no return). Used by the tests and the probe tooling.
pub fn evaluate(prog: &[Insn], data: &SeccompData) -> Option<u32> {
    let word = |off: u32| -> Option<u32> {
        match off {
            DATA_NR => Some(data.nr),
            DATA_ARCH => Some(data.arch),
            o if (DATA_ARGS..DATA_ARGS + 48).contains(&o) && o.is_multiple_of(4) => {
                let a = data.args[((o - DATA_ARGS) / 8) as usize];
                Some(if (o - DATA_ARGS).is_multiple_of(8) {
                    a as u32
                } else {
                    (a >> 32) as u32
                })
            }
            _ => None,
        }
    };
    let mut acc = 0u32;
    let mut pc = 0usize;
    loop {
        let i = prog.get(pc)?;
        pc += 1;
        let branch = |cond: bool| usize::from(if cond { i.jt } else { i.jf });
        match i.code {
            LD_W_ABS => acc = word(i.k)?,
            ALU_AND_K => acc &= i.k,
            RET_K => return Some(i.k),
            JMP_JA => pc += i.k as usize,
            JMP_JEQ_K => pc += branch(acc == i.k),
            JMP_JGT_K => pc += branch(acc > i.k),
            JMP_JGE_K => pc += branch(acc >= i.k),
            _ => return None,
        }
    }
}

#[cfg(all(feature = "seccomp", target_arch = "x86_64"))]
mod imp {
    use std::collections::BTreeMap;

    use seccompiler::{
        BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
        SeccompRule, TargetArch,
    };

    use super::*;

    /// Returned by a compiled block when no rule matched; rewritten into a jump past the
    /// block so the next block runs. `SECCOMP_RET_TRACE` with a marker value never appears
    /// in the finished program.
    const FALLTHROUGH: u32 = 0x7e57;

    type Abi = fn(&Syscall) -> Option<u32>;

    fn cond(arg: u8, op: SeccompCmpOp, v: u32) -> Result<SeccompCondition, String> {
        SeccompCondition::new(arg, SeccompCmpArgLen::Dword, op, u64::from(v))
            .map_err(|e| e.to_string())
    }

    fn rule(c: Vec<SeccompCondition>) -> Result<SeccompRule, String> {
        SeccompRule::new(c).map_err(|e| e.to_string())
    }

    /// Compile `rules` into a block that returns `action` on a match and falls through to
    /// the instruction after it otherwise.
    fn block(rules: BTreeMap<i64, Vec<SeccompRule>>, action: u32) -> Result<Vec<Insn>, String> {
        let filter = SeccompFilter::new(
            rules,
            SeccompAction::Trace(FALLTHROUGH),
            SeccompAction::Errno(action),
            TargetArch::x86_64,
        )
        .map_err(|e| e.to_string())?;
        let prog: BpfProgram = filter
            .try_into()
            .map_err(|e: seccompiler::BackendError| e.to_string())?;
        // seccompiler starts every program with `ld arch; jeq <x86_64>, 1, 0; ret kill`.
        // The dispatch below has already checked the architecture.
        let body = prog.get(3..).ok_or("short program")?;
        if prog.first().map(|i| (i.code, i.k)) != Some((LD_W_ABS, DATA_ARCH)) {
            return Err("unexpected seccompiler preamble".into());
        }
        let fallthrough = 0x7ff0_0000 | FALLTHROUGH;
        let n = body.len();
        let out: Vec<Insn> = body
            .iter()
            .enumerate()
            .map(|(idx, i)| {
                if i.code == RET_K && i.k == fallthrough {
                    Insn {
                        code: JMP_JA,
                        jt: 0,
                        jf: 0,
                        k: (n - 1 - idx) as u32,
                    }
                } else {
                    Insn {
                        code: i.code,
                        jt: i.jt,
                        jf: i.jf,
                        k: i.k,
                    }
                }
            })
            .collect();
        Ok(out)
    }

    /// The `EPERM` block and the `EAFNOSUPPORT` block of one ABI.
    fn abi_blocks(abi: Abi, profile: Profile) -> Result<Vec<Insn>, String> {
        let mut eperm: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
        for s in RULES {
            if let Some(nr) = abi(s) {
                eperm.insert(i64::from(nr), Vec::new());
            }
        }
        if let Some(nr) = abi(&IOCTL) {
            let rules = IOCTL_DENIED
                .iter()
                .map(|&req| rule(vec![cond(1, SeccompCmpOp::Eq, req)?]))
                .collect::<Result<Vec<_>, _>>()?;
            eperm.insert(i64::from(nr), rules);
        }
        if let Some(nr) = abi(&PERSONALITY) {
            let conds = PERSONALITY_ALLOWED
                .iter()
                .map(|&p| cond(0, SeccompCmpOp::Ne, p))
                .collect::<Result<Vec<_>, _>>()?;
            eperm.insert(i64::from(nr), vec![rule(conds)?]);
        }
        let mut out = block(eperm, EPERM)?;
        if let Some(nr) = abi(&SOCKET) {
            let mut rules = vec![rule(vec![
                cond(0, SeccompCmpOp::Eq, AF_NETLINK)?,
                cond(2, SeccompCmpOp::Eq, NETLINK_AUDIT)?,
            ])?];
            let packet = profile.deny_packet_sockets.then_some(AF_PACKET);
            for &fam in SOCKET_FAMILIES_DENIED.iter().chain(packet.iter()) {
                rules.push(rule(vec![cond(0, SeccompCmpOp::Eq, fam)?])?);
            }
            out.extend(block(
                BTreeMap::from([(i64::from(nr), rules)]),
                EAFNOSUPPORT,
            )?);
        }
        Ok(out)
    }

    const fn ins(code: u16, jt: u8, jf: u8, k: u32) -> Insn {
        Insn { code, jt, jf, k }
    }

    pub(super) fn compile(profile: Profile) -> Result<Vec<Insn>, String> {
        let mut x86_64 = vec![
            ins(LD_W_ABS, 0, 0, DATA_NR),
            ins(JMP_JGE_K, 0, 1, X32_SYSCALL_BIT),
            ins(RET_K, 0, 0, RET_ERRNO | ENOSYS),
        ];
        x86_64.extend(abi_blocks(|s| s.x86_64, profile)?);
        x86_64.push(ins(RET_K, 0, 0, RET_ALLOW));
        let mut i386 = abi_blocks(|s| s.i386, profile)?;
        i386.push(ins(RET_K, 0, 0, RET_ALLOW));

        // 0 ld arch; 1 jeq x86_64; 2 ja L64; 3 jeq i386; 4 ja L32; 5 ret allow; 6 L64 …
        let to_i386 = u32::try_from(x86_64.len()).map_err(|e| e.to_string())?;
        let mut prog = vec![
            ins(LD_W_ABS, 0, 0, DATA_ARCH),
            ins(JMP_JEQ_K, 0, 1, AUDIT_ARCH_X86_64),
            ins(JMP_JA, 0, 0, 3),
            ins(JMP_JEQ_K, 0, 1, AUDIT_ARCH_I386),
            ins(JMP_JA, 0, 0, 1 + to_i386),
            ins(RET_K, 0, 0, RET_ALLOW),
        ];
        prog.extend(x86_64);
        prog.extend(i386);
        // BPF_MAXINSNS.
        if prog.len() > 4096 {
            return Err(format!("program too long: {}", prog.len()));
        }
        Ok(prog)
    }
}

/// Build the program for `profile` as instructions.
pub fn instructions(profile: Profile) -> Result<Vec<Insn>, Error> {
    #[cfg(all(feature = "seccomp", target_arch = "x86_64"))]
    {
        imp::compile(profile).map_err(|detail| {
            Error::new(crate::error::ErrorCode::SbxSetupFailed)
                .with("step", "seccomp")
                .with("detail", detail)
        })
    }
    #[cfg(not(all(feature = "seccomp", target_arch = "x86_64")))]
    {
        let _ = profile;
        Err(Error::not_available("SANDBOX-seccomp"))
    }
}

/// Build the classic-BPF program for `--seccomp` (raw `struct sock_filter` bytes).
pub fn build(profile: Profile) -> Result<Vec<u8>, Error> {
    instructions(profile).map(|p| to_bytes(&p))
}

/// The cached program for `profile` (built once per process and profile).
pub fn program(profile: Profile) -> Result<&'static [u8], Error> {
    use std::sync::OnceLock;
    static CACHE: [OnceLock<Vec<u8>>; 2] = [OnceLock::new(), OnceLock::new()];
    let slot = &CACHE[usize::from(profile.deny_packet_sockets)];
    if let Some(p) = slot.get() {
        return Ok(p);
    }
    let bytes = build(profile)?;
    Ok(slot.get_or_init(|| bytes))
}

#[cfg(all(test, feature = "seccomp", target_arch = "x86_64"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn run(profile: Profile, arch: u32, nr: u32, args: [u64; 6]) -> u32 {
        let prog = instructions(profile).unwrap();
        evaluate(&prog, &SeccompData { nr, arch, args }).expect("well-formed program")
    }

    fn eperm() -> u32 {
        RET_ERRNO | EPERM
    }

    #[test]
    fn x86_64_numbers_match_libc() {
        let known: &[(&str, i64)] = &[
            ("add_key", libc::SYS_add_key),
            ("request_key", libc::SYS_request_key),
            ("keyctl", libc::SYS_keyctl),
            ("bpf", libc::SYS_bpf),
            ("perf_event_open", libc::SYS_perf_event_open),
            ("userfaultfd", libc::SYS_userfaultfd),
            ("io_uring_setup", libc::SYS_io_uring_setup),
            ("io_uring_enter", libc::SYS_io_uring_enter),
            ("io_uring_register", libc::SYS_io_uring_register),
            ("kexec_load", libc::SYS_kexec_load),
            ("kexec_file_load", libc::SYS_kexec_file_load),
            ("reboot", libc::SYS_reboot),
            ("swapon", libc::SYS_swapon),
            ("swapoff", libc::SYS_swapoff),
            ("init_module", libc::SYS_init_module),
            ("finit_module", libc::SYS_finit_module),
            ("delete_module", libc::SYS_delete_module),
            ("acct", libc::SYS_acct),
            ("quotactl", libc::SYS_quotactl),
            ("settimeofday", libc::SYS_settimeofday),
            ("clock_settime", libc::SYS_clock_settime),
            ("clock_adjtime", libc::SYS_clock_adjtime),
            ("adjtimex", libc::SYS_adjtimex),
            ("syslog", libc::SYS_syslog),
            ("uselib", libc::SYS_uselib),
            ("nfsservctl", libc::SYS_nfsservctl),
            ("vhangup", libc::SYS_vhangup),
            ("lookup_dcookie", libc::SYS_lookup_dcookie),
            ("open_by_handle_at", libc::SYS_open_by_handle_at),
            ("name_to_handle_at", libc::SYS_name_to_handle_at),
            ("ioctl", libc::SYS_ioctl),
            ("socket", libc::SYS_socket),
            ("personality", libc::SYS_personality),
        ];
        let all: Vec<Syscall> = RULES
            .iter()
            .copied()
            .chain([IOCTL, SOCKET, PERSONALITY])
            .collect();
        for (name, nr) in known {
            let s = all.iter().find(|s| s.name == *name).unwrap();
            assert_eq!(s.x86_64.map(i64::from), Some(*nr), "{name}");
        }
    }

    #[test]
    fn every_denied_call_is_eperm_in_both_abis() {
        for p in [
            Profile::default(),
            Profile {
                deny_packet_sockets: true,
            },
        ] {
            for s in RULES {
                if let Some(nr) = s.x86_64 {
                    assert_eq!(run(p, AUDIT_ARCH_X86_64, nr, [0; 6]), eperm(), "{}", s.name);
                }
                if let Some(nr) = s.i386 {
                    assert_eq!(
                        run(p, AUDIT_ARCH_I386, nr, [0; 6]),
                        eperm(),
                        "{} i386",
                        s.name
                    );
                }
            }
        }
    }

    #[test]
    fn needed_calls_stay_allowed() {
        // read, write, openat, mount, umount2, pivot_root, unshare, setns, clone, clone3,
        // ptrace, process_vm_readv/writev, chroot, memfd_create, mbind, seccomp, prctl,
        // modify_ldt.
        let x86_64 = [
            0, 1, 257, 165, 166, 155, 272, 308, 56, 435, 101, 310, 311, 161, 319, 237, 317, 157,
            154,
        ];
        let i386 = [
            3, 4, 295, 21, 52, 217, 310, 346, 120, 435, 26, 347, 348, 61, 356, 274, 354, 172, 123,
        ];
        for nr in x86_64 {
            assert_eq!(
                run(Profile::default(), AUDIT_ARCH_X86_64, nr, [0; 6]),
                RET_ALLOW,
                "{nr}"
            );
        }
        for nr in i386 {
            assert_eq!(
                run(Profile::default(), AUDIT_ARCH_I386, nr, [0; 6]),
                RET_ALLOW,
                "{nr}"
            );
        }
    }

    #[test]
    fn ioctl_terminal_injection_only() {
        for (arch, nr) in [(AUDIT_ARCH_X86_64, 16), (AUDIT_ARCH_I386, 54)] {
            let p = Profile::default();
            assert_eq!(run(p, arch, nr, [0, 0x5412, 0, 0, 0, 0]), eperm());
            assert_eq!(run(p, arch, nr, [0, 0x541C, 0, 0, 0, 0]), eperm());
            // High bits are ignored by the kernel and by the filter (CVE-2019-10063).
            assert_eq!(run(p, arch, nr, [0, 0x1_0000_5412, 0, 0, 0, 0]), eperm());
            // TCGETS, FIONREAD stay allowed.
            assert_eq!(run(p, arch, nr, [0, 0x5401, 0, 0, 0, 0]), RET_ALLOW);
            assert_eq!(run(p, arch, nr, [0, 0x541B, 0, 0, 0, 0]), RET_ALLOW);
        }
    }

    #[test]
    fn socket_families() {
        let eaf = RET_ERRNO | EAFNOSUPPORT;
        for (arch, nr) in [(AUDIT_ARCH_X86_64, 41), (AUDIT_ARCH_I386, 359)] {
            let off = Profile::default();
            let none = Profile {
                deny_packet_sockets: true,
            };
            for fam in SOCKET_FAMILIES_DENIED {
                assert_eq!(run(off, arch, nr, [u64::from(*fam), 1, 0, 0, 0, 0]), eaf);
            }
            assert_eq!(
                run(off, arch, nr, [16, 3, 9, 0, 0, 0]),
                eaf,
                "netlink audit"
            );
            assert_eq!(
                run(off, arch, nr, [16, 3, 15, 0, 0, 0]),
                RET_ALLOW,
                "uevent"
            );
            assert_eq!(run(off, arch, nr, [16, 3, 0, 0, 0, 0]), RET_ALLOW, "route");
            assert_eq!(run(off, arch, nr, [17, 3, 0, 0, 0, 0]), RET_ALLOW);
            assert_eq!(run(none, arch, nr, [17, 3, 0, 0, 0, 0]), eaf);
            for fam in [1, 2, 10] {
                assert_eq!(
                    run(none, arch, nr, [fam, 1, 0, 0, 0, 0]),
                    RET_ALLOW,
                    "{fam}"
                );
            }
        }
    }

    #[test]
    fn personality_values() {
        for (arch, nr) in [(AUDIT_ARCH_X86_64, 135), (AUDIT_ARCH_I386, 136)] {
            let p = Profile::default();
            for v in PERSONALITY_ALLOWED {
                assert_eq!(run(p, arch, nr, [u64::from(*v), 0, 0, 0, 0, 0]), RET_ALLOW);
            }
            // READ_IMPLIES_EXEC, UNAME26, PER_BSD.
            for v in [0x0040_0000u64, 0x0002_0000, 0x0006] {
                assert_eq!(run(p, arch, nr, [v, 0, 0, 0, 0, 0]), eperm(), "{v:#x}");
            }
        }
    }

    #[test]
    fn x32_abi_is_refused_and_other_arches_pass() {
        let p = Profile::default();
        // x32 read and keyctl.
        assert_eq!(
            run(p, AUDIT_ARCH_X86_64, X32_SYSCALL_BIT, [0; 6]),
            RET_ERRNO | ENOSYS
        );
        assert_eq!(
            run(p, AUDIT_ARCH_X86_64, X32_SYSCALL_BIT | 250, [0; 6]),
            RET_ERRNO | ENOSYS
        );
        // AArch64 keyctl: not an ABI of this kernel; default action.
        assert_eq!(run(p, 183 | 0xC000_0000, 219, [0; 6]), RET_ALLOW);
    }

    #[test]
    fn bytes_round_trip_and_cache() {
        let p = Profile::default();
        let bytes = program(p).unwrap();
        assert_eq!(bytes.len() % 8, 0);
        assert_eq!(from_bytes(bytes).unwrap(), instructions(p).unwrap());
        assert!(std::ptr::eq(bytes, program(p).unwrap()));
        assert_ne!(
            program(Profile {
                deny_packet_sockets: true
            })
            .unwrap(),
            bytes
        );
    }

    #[test]
    fn profile_from_args() {
        assert!(Profile::for_args(&["--unshare-user", "--unshare-net"]).deny_packet_sockets);
        assert!(!Profile::for_args(&["--unshare-user"]).deny_packet_sockets);
    }
}
