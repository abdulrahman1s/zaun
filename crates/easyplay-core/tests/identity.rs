//! Identity: fingerprints, PE inspection, matching, game roots, shortcuts, hints
//! (GAME_IDENTITY, IDENT-001 … IDENT-007).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use easyplay_core::clock::{Clock, SystemClock, Timestamp};
use easyplay_core::consts::QUICK_HASH_WINDOW;
use easyplay_core::identity::fingerprint::{self, FullHash};
use easyplay_core::identity::game_root::{self, RootChoiceError};
use easyplay_core::identity::hints::{self, AntiCheat};
use easyplay_core::identity::matcher::{self, Change};
use easyplay_core::identity::pe::{self, note};
use easyplay_core::identity::{
    ExeFormat, FsPeInspector, IndexedExe, LibraryIndex, MatchOutcome, PathProbe, PeError,
    PeInspector, PeLimits, RootKind, RootReason, SharedLocations, SharedReason, UnknownHint, lnk,
};
use easyplay_core::ids::{ExecutableId, GameId};
use easyplay_core::jobs::CancelToken;
use easyplay_core::model::{Fingerprint, PeInfo, PeMachine, PeSubsystem, Sha256};
use easyplay_testkit::MemFs;
use easyplay_testkit::fixtures::games::{exe_id, game_id};
use easyplay_testkit::fixtures::lnk::{LnkSpec, build as build_lnk};
use easyplay_testkit::fixtures::pe::{PeBuilder, malformed_corpus, sample_game};
use sha2::{Digest, Sha256 as Hasher};

const W: u64 = QUICK_HASH_WINDOW;

fn bytes(n: u64) -> Vec<u8> {
    (0..n).map(|i| (i * 7 % 256) as u8).collect()
}

// ---- Fingerprint --------------------------------------------------------------------------

fn expected_quick(data: &[u8]) -> Sha256 {
    let size = data.len() as u64;
    let mut h = Hasher::new();
    h.update(size.to_le_bytes());
    h.update(&data[..size.min(W) as usize]);
    if size > 2 * W {
        h.update(&data[(size - W) as usize..]);
    }
    h.finalize().into()
}

#[test]
fn quick_hash_boundary_table() {
    let fs = MemFs::new();
    for size in [
        0,
        1,
        97,
        W - 1,
        W,
        W + 1,
        2 * W - 1,
        2 * W,
        2 * W + 1,
        3 * W + 5,
    ] {
        let p = PathBuf::from(format!("/g/f{size}.exe"));
        let data = bytes(size);
        fs.file(&p, &data);
        fs.set_mtime(&p, 1234);
        let fp = fingerprint::compute(&fs, &p).unwrap();
        assert_eq!(fp.size, size);
        assert_eq!(fp.mtime_ns, 1234);
        assert_eq!(fp.full_hash, None);
        assert_eq!(fp.quick_hash, expected_quick(&data), "size {size}");
    }
}

#[test]
fn quick_hash_ignores_the_middle_and_sees_the_tail() {
    let fs = MemFs::new();
    let mut a = bytes(3 * W);
    fs.file("/a.exe", &a);
    let base = fingerprint::compute(&fs, Path::new("/a.exe")).unwrap();
    a[(W + 10) as usize] ^= 0xff; // middle
    fs.file("/a.exe", &a);
    assert_eq!(
        fingerprint::compute(&fs, Path::new("/a.exe"))
            .unwrap()
            .quick_hash,
        base.quick_hash
    );
    let last = a.len() - 1;
    a[last] ^= 0xff; // tail
    fs.file("/a.exe", &a);
    assert_ne!(
        fingerprint::compute(&fs, Path::new("/a.exe"))
            .unwrap()
            .quick_hash,
        base.quick_hash
    );
}

#[test]
fn unchanged_shortcut_and_refresh() {
    let fs = MemFs::new();
    fs.file("/g/x.exe", bytes(500));
    fs.set_mtime("/g/x.exe", 10);
    let fp = fingerprint::compute(&fs, Path::new("/g/x.exe")).unwrap();
    let with_full = Fingerprint {
        full_hash: Some([1; 32]),
        ..fp
    };
    let (same, changed) = fingerprint::refresh(&fs, Path::new("/g/x.exe"), &with_full).unwrap();
    assert!(!changed);
    assert_eq!(same, with_full);
    fs.set_mtime("/g/x.exe", 11);
    let (fresh, changed) = fingerprint::refresh(&fs, Path::new("/g/x.exe"), &with_full).unwrap();
    assert!(changed);
    assert_eq!(fresh.full_hash, None);
    assert_eq!(fresh.quick_hash, fp.quick_hash);
}

#[test]
fn fingerprint_refuses_symlinks_and_dirs() {
    let fs = MemFs::new();
    fs.file("/g/x.exe", b"MZ")
        .link("/g/l.exe", "/g/x.exe")
        .dir("/g/d");
    assert!(fingerprint::compute(&fs, Path::new("/g/l.exe")).is_err());
    assert!(fingerprint::compute(&fs, Path::new("/g/d")).is_err());
}

#[test]
fn full_hash_limits_and_cancellation() {
    let fs = MemFs::new();
    let data = bytes(3 * W + 3);
    fs.file("/x.exe", &data);
    let tok = CancelToken::new();
    let want: Sha256 = Hasher::digest(&data).into();
    assert_eq!(
        fingerprint::full_hash(&fs, Path::new("/x.exe"), 10 * W, &tok).unwrap(),
        FullHash::Done(want)
    );
    assert_eq!(
        fingerprint::full_hash(&fs, Path::new("/x.exe"), W, &tok).unwrap(),
        FullHash::TooLarge
    );
    tok.cancel();
    assert_eq!(
        fingerprint::full_hash(&fs, Path::new("/x.exe"), 10 * W, &tok).unwrap(),
        FullHash::Cancelled
    );
}

// ---- PE -----------------------------------------------------------------------------------

fn inspector(fs: &Arc<MemFs>) -> FsPeInspector {
    FsPeInspector::new(fs.clone(), Arc::new(SystemClock::new()))
}

#[test]
fn inspects_generated_game() {
    let fs = Arc::new(MemFs::new());
    fs.file("/g/hk.exe", sample_game());
    let i = inspector(&fs);
    let info = i
        .inspect(Path::new("/g/hk.exe"), &PeLimits::default())
        .unwrap();
    assert_eq!(info.product_name.as_deref(), Some("Hollow Knight"));
    assert_eq!(info.file_description.as_deref(), Some("Hollow Knight"));
    assert_eq!(info.company.as_deref(), Some("Team Cherry"));
    assert_eq!(info.original_filename.as_deref(), Some("hollow_knight.exe"));
    assert_eq!(info.file_version.as_deref(), Some("1.5.78.11833"));
    assert_eq!(info.machine, PeMachine::X86_64);
    assert_eq!(info.subsystem, PeSubsystem::Gui);
    assert_eq!(info.timestamp, Some(0x5F00_0000));
    assert!(info.has_icon && !info.is_dll && !info.is_dotnet);
    assert!(info.notes.is_empty(), "{:?}", info.notes);

    let icon = i
        .largest_icon(Path::new("/g/hk.exe"), &PeLimits::default())
        .unwrap()
        .unwrap();
    assert_eq!((icon.width, icon.height, icon.bit_count), (256, 256, 32));
    assert_eq!(icon.ico.len(), 22 + 2000);
    assert_eq!(&icon.ico[..6], &[0, 0, 1, 0, 1, 0]);

    let imports = i
        .imports(Path::new("/g/hk.exe"), &PeLimits::default())
        .unwrap();
    assert_eq!(imports, vec!["kernel32.dll", "winhttp.dll", "ws2_32.dll"]);
}

#[test]
fn flags_and_notes() {
    let l = PeLimits::default();
    let dll = pe::inspect_bytes(&PeBuilder::new().dll().clr().build(), &l).unwrap();
    assert!(dll.is_dll && dll.is_dotnet);
    assert!(dll.notes.contains(&note::DLL.to_owned()));
    assert!(dll.notes.contains(&note::DOTNET.to_owned()));
    let drv = pe::inspect_bytes(&PeBuilder::new().subsystem(1).build(), &l).unwrap();
    assert!(drv.notes.contains(&note::DRIVER.to_owned()));
    let con = pe::inspect_bytes(&PeBuilder::new().x86().subsystem(3).build(), &l).unwrap();
    assert_eq!(
        (con.machine, con.subsystem),
        (PeMachine::X86, PeSubsystem::Console)
    );
    let arm = pe::inspect_bytes(&PeBuilder::new().machine(0xaa64).build(), &l).unwrap();
    assert_eq!(arm.machine, PeMachine::Arm64);
    assert!(arm.notes.contains(&note::UNUSUAL_MACHINE.to_owned()));
    let bind = pe::inspect_bytes(&PeBuilder::new().bind_section().build(), &l).unwrap();
    assert!(bind.notes.contains(&note::STEAM_DRM.to_owned()));
    let none = pe::inspect_bytes(&PeBuilder::new().build(), &l).unwrap();
    assert!(!none.has_icon && none.product_name.is_none());
    assert_eq!(
        pe::largest_icon_bytes(&PeBuilder::new().build(), &l).unwrap(),
        None
    );
}

#[test]
fn strings_are_sanitised_and_capped() {
    let long = "x".repeat(1000);
    let b = PeBuilder::new()
        .string("ProductName", "\u{202E}Evil\u{0007}Game  ")
        .string("CompanyName", &long)
        .build();
    let info = pe::inspect_bytes(&b, &PeLimits::default()).unwrap();
    assert_eq!(info.product_name.as_deref(), Some("EvilGame"));
    assert_eq!(info.company.as_deref().map(str::len), Some(256));
}

#[test]
fn icon_and_resource_limits() {
    let b = PeBuilder::new()
        .icon(32, 32, 32, 100)
        .icon(256, 256, 32, 5000)
        .build();
    let small = PeLimits {
        max_icon_bytes: 1000,
        ..PeLimits::default()
    };
    let icon = pe::largest_icon_bytes(&b, &small).unwrap().unwrap();
    assert_eq!(icon.width, 32, "oversized icon skipped");
    let tight = PeLimits {
        max_resource_entries: 2,
        ..PeLimits::default()
    };
    let info = pe::inspect_bytes(&sample_game(), &tight).unwrap();
    assert!(info.notes.contains(&note::RESOURCES_OVER_LIMIT.to_owned()));
    assert_eq!(info.product_name, None);
    let truncated = PeLimits {
        max_map_bytes: 64,
        ..PeLimits::default()
    };
    let info = pe::inspect_bytes(&sample_game(), &truncated).unwrap();
    assert!(info.notes.contains(&note::RESOURCES_TRUNCATED.to_owned()));
}

#[test]
fn deadline_is_enforced() {
    let fs = Arc::new(MemFs::new());
    fs.file("/g/hk.exe", sample_game());
    let l = PeLimits {
        deadline: Duration::ZERO,
        ..PeLimits::default()
    };
    assert_eq!(
        inspector(&fs).inspect(Path::new("/g/hk.exe"), &l),
        Err(PeError::DeadlineExceeded)
    );
}

#[test]
fn malformed_corpus_never_panics() {
    let l = PeLimits::default();
    let fs = Arc::new(MemFs::new());
    let i = inspector(&fs);
    for (label, b) in malformed_corpus() {
        let p = PathBuf::from("/m.exe");
        fs.file(&p, &b);
        let clock = SystemClock::new();
        let started = clock.mono();
        for r in [
            i.inspect(&p, &l).map(|_| ()),
            i.largest_icon(&p, &l).map(|_| ()),
            i.imports(&p, &l).map(|_| ()),
        ] {
            assert_ne!(r, Err(PeError::Panicked), "{label}");
        }
        assert!(
            clock.mono().saturating_duration_since(started) < Duration::from_secs(2),
            "{label}"
        );
    }
    let cyclic = malformed_corpus()
        .into_iter()
        .find(|(l, _)| l == "cyclic resource directory")
        .unwrap()
        .1;
    let info = pe::inspect_bytes(&cyclic, &l).unwrap();
    assert!(info.notes.contains(&note::RESOURCES_OVER_LIMIT.to_owned()));
}

#[test]
fn dos_and_ne_are_flagged() {
    let fs = Arc::new(MemFs::new());
    let mut ne = vec![0u8; 0x100];
    ne[..2].copy_from_slice(b"MZ");
    ne[0x3c] = 0x80;
    ne[0x80..0x82].copy_from_slice(b"NE");
    fs.file("/ne.exe", &ne)
        .file("/dos.exe", b"MZ\x90\x00")
        .file("/txt.exe", b"hello");
    let i = inspector(&fs);
    assert_eq!(i.format(Path::new("/ne.exe")), Ok(ExeFormat::Ne));
    assert_eq!(i.format(Path::new("/dos.exe")), Ok(ExeFormat::DosOnly));
    assert_eq!(i.format(Path::new("/txt.exe")), Err(PeError::NotExecutable));
    assert_eq!(
        i.inspect(Path::new("/dos.exe"), &PeLimits::default()),
        Err(PeError::NotPe(ExeFormat::DosOnly))
    );
    assert_eq!(PeInfo::unknown(), PeInfo::default());
}

// ---- Matcher ------------------------------------------------------------------------------

#[derive(Default)]
struct Index {
    exes: Vec<IndexedExe>,
    roots: Vec<(PathBuf, GameId)>,
}

impl LibraryIndex for Index {
    fn by_path(&self, path: &Path) -> Option<IndexedExe> {
        self.exes.iter().find(|e| e.canonical_path == path).cloned()
    }
    fn by_hash(&self, hash: &Sha256) -> Vec<IndexedExe> {
        self.exes
            .iter()
            .filter(|e| e.fingerprint.quick_hash == *hash)
            .cloned()
            .collect()
    }
    fn roots(&self) -> Vec<(PathBuf, GameId)> {
        self.roots.clone()
    }
}

#[derive(Default)]
struct Probe {
    existing: Vec<PathBuf>,
    unmounted: Vec<PathBuf>,
}

impl PathProbe for Probe {
    fn exists(&self, path: &Path) -> bool {
        self.existing.iter().any(|p| p == path)
    }
    fn is_mounted(&self, mp: &Path) -> bool {
        !self.unmounted.iter().any(|p| p == mp)
    }
}

fn fp(h: u8) -> Fingerprint {
    Fingerprint {
        size: 10,
        mtime_ns: 1,
        quick_hash: [h; 32],
        full_hash: None,
    }
}

fn product(p: &str) -> PeInfo {
    PeInfo {
        product_name: Some(p.into()),
        ..PeInfo::default()
    }
}

fn ie(g: u32, k: u32, path: &str, h: u8, pe: PeInfo, played: Option<i64>) -> IndexedExe {
    IndexedExe {
        game: game_id(g),
        exe: exe_id(g, k),
        canonical_path: path.into(),
        mount_point: Some("/".into()),
        fingerprint: fp(h),
        pe,
        last_played_at: played.map(Timestamp::from_millis),
    }
}

struct Case {
    name: &'static str,
    exes: Vec<IndexedExe>,
    roots: Vec<(&'static str, u32)>,
    existing: Vec<&'static str>,
    unmounted: Vec<&'static str>,
    path: &'static str,
    hash: u8,
    pe: PeInfo,
    want: MatchOutcome,
}

fn g(n: u32) -> GameId {
    game_id(n)
}
fn x(n: u32, k: u32) -> ExecutableId {
    exe_id(n, k)
}

#[test]
fn matcher_rule_table() {
    let hk = || product("Hollow Knight");
    let cases = vec![
        Case {
            name: "rule 1 known",
            exes: vec![ie(1, 0, "/g/hk/hk.exe", 1, hk(), None)],
            roots: vec![("/g/hk", 1)],
            existing: vec!["/g/hk/hk.exe"],
            unmounted: vec![],
            path: "/g/hk/hk.exe",
            hash: 1,
            pe: hk(),
            want: MatchOutcome::Known {
                game: g(1),
                exe: x(1, 0),
            },
        },
        Case {
            name: "rule 2 update",
            exes: vec![ie(1, 0, "/g/hk/hk.exe", 1, hk(), None)],
            roots: vec![],
            existing: vec![],
            unmounted: vec![],
            path: "/g/hk/hk.exe",
            hash: 2,
            pe: hk(),
            want: MatchOutcome::KnownChanged {
                game: g(1),
                exe: x(1, 0),
                change: Change::SameIdentity,
            },
        },
        Case {
            name: "rule 3 replaced",
            exes: vec![ie(1, 0, "/g/hk/hk.exe", 1, hk(), None)],
            roots: vec![],
            existing: vec![],
            unmounted: vec![],
            path: "/g/hk/hk.exe",
            hash: 2,
            pe: product("Other"),
            want: MatchOutcome::KnownChanged {
                game: g(1),
                exe: x(1, 0),
                change: Change::IdentityDiffers,
            },
        },
        Case {
            name: "rule 3 no version resources",
            exes: vec![ie(1, 0, "/g/hk/hk.exe", 1, PeInfo::default(), None)],
            roots: vec![],
            existing: vec![],
            unmounted: vec![],
            path: "/g/hk/hk.exe",
            hash: 2,
            pe: PeInfo::default(),
            want: MatchOutcome::KnownChanged {
                game: g(1),
                exe: x(1, 0),
                change: Change::IdentityDiffers,
            },
        },
        Case {
            name: "rule 4 moved",
            exes: vec![ie(1, 0, "/old/hk.exe", 1, hk(), None)],
            roots: vec![],
            existing: vec![],
            unmounted: vec![],
            path: "/new/hk.exe",
            hash: 1,
            pe: hk(),
            want: MatchOutcome::MovedCandidate {
                game: g(1),
                exe: x(1, 0),
            },
        },
        Case {
            name: "rule 4 before rule 6",
            exes: vec![ie(1, 0, "/old/hk.exe", 1, hk(), None)],
            roots: vec![("/g/celeste", 2)],
            existing: vec![],
            unmounted: vec![],
            path: "/g/celeste/hk.exe",
            hash: 1,
            pe: hk(),
            want: MatchOutcome::MovedCandidate {
                game: g(1),
                exe: x(1, 0),
            },
        },
        Case {
            name: "rule 4 not when the drive is disconnected: copy",
            exes: vec![IndexedExe {
                mount_point: Some("/run/media/s/usb".into()),
                ..ie(1, 0, "/run/media/s/usb/hk.exe", 1, hk(), None)
            }],
            roots: vec![],
            existing: vec![],
            unmounted: vec!["/run/media/s/usb"],
            path: "/g/hk.exe",
            hash: 1,
            pe: hk(),
            want: MatchOutcome::CopyCandidate {
                game: g(1),
                exe: x(1, 0),
            },
        },
        Case {
            name: "rule 5 copy picks most recently played",
            exes: vec![
                ie(1, 0, "/a/hk.exe", 1, hk(), Some(100)),
                ie(2, 0, "/b/hk.exe", 1, hk(), Some(200)),
                ie(3, 0, "/c/hk.exe", 1, hk(), None),
            ],
            roots: vec![],
            existing: vec!["/a/hk.exe", "/b/hk.exe", "/c/hk.exe"],
            unmounted: vec![],
            path: "/d/hk.exe",
            hash: 1,
            pe: hk(),
            want: MatchOutcome::CopyCandidate {
                game: g(2),
                exe: x(2, 0),
            },
        },
        Case {
            name: "rule 6 deepest root",
            exes: vec![],
            roots: vec![("/g", 1), ("/g/hk", 2), ("/g/hk2", 3)],
            existing: vec![],
            unmounted: vec![],
            path: "/g/hk/tools/cfg.exe",
            hash: 9,
            pe: PeInfo::default(),
            want: MatchOutcome::InsideKnownRoot { game: g(2) },
        },
        Case {
            name: "rule 7 ambiguous move",
            exes: vec![
                ie(1, 0, "/a/x.exe", 1, hk(), None),
                ie(2, 0, "/b/x.exe", 1, hk(), None),
            ],
            roots: vec![],
            existing: vec![],
            unmounted: vec![],
            path: "/c/x.exe",
            hash: 1,
            pe: hk(),
            want: MatchOutcome::Unknown {
                hint: Some(UnknownHint::AmbiguousMove {
                    candidates: vec![(g(1), x(1, 0)), (g(2), x(2, 0))],
                }),
            },
        },
        Case {
            name: "rule 5 before rule 7",
            exes: vec![
                ie(1, 0, "/a/x.exe", 1, hk(), None),
                ie(2, 0, "/b/x.exe", 1, hk(), None),
                ie(3, 0, "/e/x.exe", 1, hk(), None),
            ],
            roots: vec![],
            existing: vec!["/e/x.exe"],
            unmounted: vec![],
            path: "/c/x.exe",
            hash: 1,
            pe: hk(),
            want: MatchOutcome::CopyCandidate {
                game: g(3),
                exe: x(3, 0),
            },
        },
        Case {
            name: "rule 8 unknown",
            exes: vec![ie(1, 0, "/a/x.exe", 1, hk(), None)],
            roots: vec![("/a", 1)],
            existing: vec!["/a/x.exe"],
            unmounted: vec![],
            path: "/z/y.exe",
            hash: 7,
            pe: hk(),
            want: MatchOutcome::Unknown { hint: None },
        },
        Case {
            name: "names are not evidence",
            exes: vec![ie(1, 0, "/a/game.exe", 1, hk(), None)],
            roots: vec![],
            existing: vec!["/a/game.exe"],
            unmounted: vec![],
            path: "/b/game.exe",
            hash: 2,
            pe: hk(),
            want: MatchOutcome::Unknown { hint: None },
        },
    ];
    for c in cases {
        let index = Index {
            exes: c.exes,
            roots: c
                .roots
                .iter()
                .map(|(p, n)| (PathBuf::from(p), g(*n)))
                .collect(),
        };
        let probe = Probe {
            existing: c.existing.iter().map(PathBuf::from).collect(),
            unmounted: c.unmounted.iter().map(PathBuf::from).collect(),
        };
        let got = matcher::resolve(Path::new(c.path), &fp(c.hash), &c.pe, &index, &probe);
        assert_eq!(got, c.want, "{}", c.name);
    }
}

#[test]
fn full_hash_breaks_quick_hash_ties() {
    let mut a = ie(1, 0, "/a/x.exe", 1, PeInfo::default(), None);
    a.fingerprint.full_hash = Some([5; 32]);
    let index = Index {
        exes: vec![a],
        roots: vec![],
    };
    let probe = Probe {
        existing: vec!["/a/x.exe".into()],
        unmounted: vec![],
    };
    let mut f = fp(1);
    f.full_hash = Some([6; 32]);
    assert_eq!(
        matcher::resolve(
            Path::new("/b/x.exe"),
            &f,
            &PeInfo::default(),
            &index,
            &probe
        ),
        MatchOutcome::Unknown { hint: None }
    );
}

// ---- Game root ----------------------------------------------------------------------------

fn shared() -> SharedLocations {
    SharedLocations {
        home: Some("/home/sam".into()),
        user_dirs: vec![("Downloads".into(), "/home/sam/Downloads".into())],
        temp_dirs: vec!["/tmp".into()],
        mount_points: vec!["/".into(), "/run/media/sam/Drive".into()],
        media_parents: vec!["/run/media/sam".into(), "/mnt".into()],
        easyplay_dirs: vec!["/home/sam/.local/share/easyplay".into()],
        game_roots: vec!["/home/sam/Games/Celeste".into()],
        deny: None,
    }
}

#[test]
fn game_root_fixture_trees() {
    let fs = MemFs::new();
    fs.file("/home/sam/Games/Hollow Knight/hollow_knight.exe", b"MZ")
        .file(
            "/home/sam/Games/Stray/Hk_project/Binaries/Win64/Stray-Win64-Shipping.exe",
            b"MZ",
        )
        .dir("/home/sam/Games/Stray/Engine")
        .file("/home/sam/Games/Old/bin/x64/old.exe", b"MZ")
        .file("/home/sam/Games/Gdk/Binaries/WinGDK/gdk.exe", b"MZ")
        .file("/home/sam/Downloads/setup.exe", b"MZ")
        .file("/run/media/sam/Drive/game.exe", b"MZ")
        .file("/run/media/sam/Drive/bin/game.exe", b"MZ")
        .file("/home/sam/Games/Celeste/Celeste.exe", b"MZ")
        .file("/home/sam/Games/Celeste/tools/editor.exe", b"MZ")
        .file("/home/sam/bin/tool.exe", b"MZ")
        .link(
            "/home/sam/Desktop/hk.exe",
            "/home/sam/Games/Hollow Knight/hollow_knight.exe",
        );
    let s = shared();
    let detect = |p: &str| game_root::detect(&fs, &s, Path::new(p)).unwrap();

    let flat = detect("/home/sam/Games/Hollow Knight/hollow_knight.exe");
    assert_eq!(flat.kind, RootKind::Folder);
    assert_eq!(flat.root, Some("/home/sam/Games/Hollow Knight".into()));
    assert_eq!(flat.reason, RootReason::ExecutableFolder);

    let unreal = detect("/home/sam/Games/Stray/Hk_project/Binaries/Win64/Stray-Win64-Shipping.exe");
    assert_eq!(unreal.reason, RootReason::UnrealLayout);
    assert_eq!(unreal.root, Some("/home/sam/Games/Stray".into()));
    assert_eq!(
        unreal.candidates,
        vec![
            PathBuf::from("/home/sam/Games/Stray/Hk_project/Binaries/Win64"),
            "/home/sam/Games/Stray/Hk_project/Binaries".into(),
            "/home/sam/Games/Stray/Hk_project".into(),
            "/home/sam/Games/Stray".into(),
        ]
    );

    let nested = detect("/home/sam/Games/Old/bin/x64/old.exe");
    assert_eq!(nested.reason, RootReason::BinaryDirs);
    assert_eq!(nested.root, Some("/home/sam/Games/Old".into()));

    let gdk = detect("/home/sam/Games/Gdk/Binaries/WinGDK/gdk.exe");
    assert_eq!(gdk.root, Some("/home/sam/Games/Gdk".into()));

    let dl = detect("/home/sam/Downloads/setup.exe");
    assert_eq!(dl.kind, RootKind::LooseFile);
    assert_eq!(
        dl.reason,
        RootReason::Shared(SharedReason::UserDir("Downloads".into()))
    );
    assert_eq!(dl.root, None);

    let drive = detect("/run/media/sam/Drive/game.exe");
    assert_eq!(drive.reason, RootReason::Shared(SharedReason::MountRoot));

    let drive_bin = detect("/run/media/sam/Drive/bin/game.exe");
    assert_eq!(
        drive_bin.root,
        Some("/run/media/sam/Drive/bin".into()),
        "never climbs to a drive root"
    );

    let other = detect("/home/sam/Games/Celeste/Celeste.exe");
    assert_eq!(
        other.reason,
        RootReason::Shared(SharedReason::OtherGameRoot)
    );
    let inside = detect("/home/sam/Games/Celeste/tools/editor.exe");
    assert_eq!(inside.root, Some("/home/sam/Games/Celeste/tools".into()));

    let home_bin = detect("/home/sam/bin/tool.exe");
    assert_eq!(
        home_bin.root,
        Some("/home/sam/bin".into()),
        "never climbs into $HOME"
    );

    let via_link = detect("/home/sam/Desktop/hk.exe");
    assert_eq!(via_link.root, Some("/home/sam/Games/Hollow Knight".into()));

    assert_eq!(
        game_root::check_choice(
            &s,
            Path::new("/home/sam/Games/Old/bin/x64/old.exe"),
            Path::new("/home/sam/Games")
        ),
        Ok(())
    );
    assert_eq!(
        game_root::check_choice(
            &s,
            Path::new("/home/sam/Games/Old/old.exe"),
            Path::new("/home/sam")
        ),
        Err(RootChoiceError::Shared(SharedReason::Home))
    );
    assert_eq!(
        game_root::check_choice(
            &s,
            Path::new("/home/sam/Games/Old/old.exe"),
            Path::new("/srv")
        ),
        Err(RootChoiceError::NotAnAncestor)
    );
}

#[test]
fn breadth_is_bounded() {
    let fs = MemFs::new();
    for i in 0..30 {
        fs.file(format!("/g/d{i}/f.bin"), b"x");
    }
    let clock = SystemClock::new();
    let (n, complete) = game_root::breadth(&fs, &clock, Path::new("/g")).unwrap();
    assert_eq!((n, complete), (60, true));
}

#[test]
fn shared_locations_from_environment() {
    let fs = MemFs::new();
    fs.file(
        "/home/sam/.config/user-dirs.dirs",
        b"XDG_DOWNLOAD_DIR=\"$HOME/T\xc3\xa9l\xc3\xa9chargements\"\n",
    );
    let env = easyplay_core::paths::PathEnv::new(1000)
        .with("HOME", "/home/sam")
        .with("USER", "sam")
        .with("XDG_RUNTIME_DIR", "/run/user/1000");
    let paths = easyplay_core::paths::AppPaths::resolve(&env).unwrap();
    let mounts = easyplay_core::fsx::MountTable::parse(
        b"22 1 0:21 / / rw - ext4 /dev/sda1 rw\n30 22 0:30 / /run/media/sam/USB rw - vfat /dev/sdb1 rw\n",
    );
    let s = SharedLocations::gather(&fs, &env, &paths, &mounts);
    assert_eq!(
        s.classify(Path::new("/home/sam/Téléchargements")),
        Some(SharedReason::UserDir("Downloads".into()))
    );
    assert_eq!(
        s.classify(Path::new("/home/sam/Downloads")),
        Some(SharedReason::UserDir("Downloads".into()))
    );
    assert_eq!(
        s.classify(Path::new("/run/media/sam/USB")),
        Some(SharedReason::MountRoot)
    );
    assert_eq!(
        s.classify(Path::new("/run/media/sam")),
        Some(SharedReason::MediaParent)
    );
    assert_eq!(
        s.classify(Path::new("/run/user/1000")),
        Some(SharedReason::Temporary)
    );
    assert_eq!(s.classify(Path::new("/home/sam/Games/X")), None);
}

// ---- Shortcuts ----------------------------------------------------------------------------

#[test]
fn shortcuts_resolve_inside_the_prefix() {
    let fs = MemFs::new();
    let pfx = Path::new("/p/prefix");
    fs.file("/p/prefix/drive_c/Program Files/My Game/Game.exe", b"MZ");
    let ansi = build_lnk(&LnkSpec {
        local_base_path: Some("C:\\PROGRAM FILES\\my game\\game.exe".into()),
        working_dir: Some("C:\\Program Files\\My Game".into()),
        arguments: Some("-windowed".into()),
        id_list: true,
        ..LnkSpec::default()
    });
    let lnk_path = Path::new("/p/prefix/drive_c/users/steamuser/Desktop/Game.lnk");
    fs.file(lnk_path, &ansi);
    let t = lnk::read(&fs, lnk_path).unwrap();
    assert_eq!(
        t.local_path.as_deref(),
        Some("C:\\PROGRAM FILES\\my game\\game.exe")
    );
    assert_eq!(t.arguments.as_deref(), Some("-windowed"));
    assert_eq!(
        lnk::resolve_target(&fs, pfx, lnk_path, &t),
        Some(PathBuf::from(
            "/p/prefix/drive_c/Program Files/My Game/Game.exe"
        ))
    );

    let uni = build_lnk(&LnkSpec {
        local_base_path: Some("C:\\Program Files\\My Game\\Game.exe".into()),
        unicode_link_info: true,
        relative_path: Some("..\\..\\..\\Program Files\\My Game\\Game.exe".into()),
        unicode: true,
        ..LnkSpec::default()
    });
    let t = lnk::parse(&uni).unwrap();
    assert_eq!(
        t.local_path.as_deref(),
        Some("C:\\Program Files\\My Game\\Game.exe")
    );
    assert!(t.relative_path.is_some());
    let rel_only = lnk::LnkTarget {
        local_path: None,
        ..t.clone()
    };
    assert_eq!(
        lnk::resolve_target(&fs, pfx, lnk_path, &rel_only),
        Some(PathBuf::from(
            "/p/prefix/drive_c/Program Files/My Game/Game.exe"
        ))
    );

    // Targets outside the prefix or through symlinks are refused.
    let host = build_lnk(&LnkSpec {
        local_base_path: Some("Z:\\home\\sam\\.bashrc".into()),
        ..LnkSpec::default()
    });
    let t = lnk::parse(&host).unwrap();
    assert_eq!(lnk::resolve_target(&fs, pfx, lnk_path, &t), None);
    fs.link("/p/prefix/drive_c/evil", "/home/sam");
    fs.file("/home/sam/x.exe", b"MZ");
    let evil = build_lnk(&LnkSpec {
        local_base_path: Some("C:\\evil\\x.exe".into()),
        ..LnkSpec::default()
    });
    let t = lnk::parse(&evil).unwrap();
    assert_eq!(lnk::resolve_target(&fs, pfx, lnk_path, &t), None);
}

#[test]
fn malformed_shortcuts_fail_cleanly() {
    let good = build_lnk(&LnkSpec {
        local_base_path: Some("C:\\G\\g.exe".into()),
        relative_path: Some("g.exe".into()),
        unicode: true,
        id_list: true,
        ..LnkSpec::default()
    });
    for cut in 0..good.len() {
        let _ = lnk::parse(&good[..cut]);
    }
    let mut state = 7u64;
    for _ in 0..2000 {
        let mut b = good.clone();
        for _ in 0..4 {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let pos = (state >> 33) as usize % b.len();
            b[pos] = (state >> 20) as u8;
        }
        let _ = lnk::parse(&b);
    }
    let fs = MemFs::new();
    fs.file("/big.lnk", vec![0u8; 2 * 1024 * 1024]);
    assert_eq!(
        lnk::read(&fs, Path::new("/big.lnk")),
        Err(lnk::LnkError::TooLarge)
    );
}

// ---- Hints --------------------------------------------------------------------------------

#[test]
fn folder_hints_and_steam_appid() {
    let fs = MemFs::new();
    fs.file("/g/Game/game.exe", sample_game())
        .file("/g/Game/steam_api64.dll", b"x")
        .file("/g/Game/steam_appid.txt", b"367520\n")
        .dir("/g/Game/EasyAntiCheat")
        .file("/g/Link/steam_appid.txt", b"1")
        .link("/g/Linked/steam_appid.txt", "/g/Link/steam_appid.txt")
        .file("/g/Big/steam_appid.txt", vec![b'1'; 100]);
    let clock = SystemClock::new();
    let names = hints::list_names(&fs, &clock, Path::new("/g/Game"));
    assert_eq!(names.len(), 4);
    let info = pe::inspect_bytes(&sample_game(), &PeLimits::default()).unwrap();
    let imports = pe::imports_bytes(&sample_game(), &PeLimits::default()).unwrap();
    let h = hints::setup_hints(&hints::HintInput {
        file_name: "game.exe",
        size: 1,
        pe: &info,
        imports: &imports,
        siblings: &names,
        loose: false,
        inside_known_root: false,
    });
    assert!(h.online_capable && !h.installer_like);
    assert_eq!(h.default_role, None);
    assert_eq!(h.exe.anti_cheat, vec![AntiCheat::EasyAntiCheat]);
    assert!(h.exe.steam_api);
    assert_eq!(
        hints::read_steam_appid(&fs, Path::new("/g/Game")),
        Some(367520)
    );
    assert_eq!(hints::read_steam_appid(&fs, Path::new("/g/Link")), Some(1));
    assert_eq!(
        hints::read_steam_appid(&fs, Path::new("/g/Linked")),
        None,
        "symlink"
    );
    assert_eq!(
        hints::read_steam_appid(&fs, Path::new("/g/Big")),
        None,
        "too large"
    );
    assert_eq!(hints::read_steam_appid(&fs, Path::new("/g/None")), None);
}
