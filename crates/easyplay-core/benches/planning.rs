//! Pure-planning budgets of PERFORMANCE §1 (PERF-001): sandbox planning and rendering,
//! policy validation and explanation, quick fingerprint, PE inspection and thumbnails.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use easyplay_core::artwork::codec::{ImageCodec, ImageCrateCodec, RgbaImage};
use easyplay_core::artwork::thumbs::{ThumbSize, scale_to};
use easyplay_core::clock::Timestamp;
use easyplay_core::fsx::{Fs, RealFs};
use easyplay_core::identity::fingerprint;
use easyplay_core::identity::pe::{PeLimits, inspect_bytes, largest_icon_bytes};
use easyplay_core::model::{FolderAccess, FolderGrant, GrantOrigin, SandboxPolicy};
use easyplay_core::sandbox::explain::{PolicyContext, explain_policy};
use easyplay_core::sandbox::{offline, plan, render};
use easyplay_testkit::bench::{Bench, finish};
use easyplay_testkit::fixtures::hosts::{HOME, SandboxWorld};
use easyplay_testkit::fixtures::pe::sample_game;

fn many_grants(w: &SandboxWorld) -> SandboxPolicy {
    let mut p = offline();
    for i in 0..24 {
        let dir = PathBuf::from(format!("{HOME}/Games/Shared {i}"));
        w.fs.dir(&dir);
        p.folders.push(FolderGrant {
            path: dir.clone(),
            canonical: dir,
            access: if i % 2 == 0 {
                FolderAccess::ReadOnly
            } else {
                FolderAccess::ReadWrite
            },
            origin: GrantOrigin::UserChosen,
            drive_letter: None,
            granted_at: Timestamp::from_millis(0),
        });
    }
    p
}

fn pctx(w: &SandboxWorld) -> PolicyContext<'_> {
    PolicyContext {
        game: w.game_id,
        root: w.root.as_deref(),
        exe: &w.exe,
        home: Path::new(HOME),
        uid: 1000,
        gid: 1000,
        user_name: "sam",
        group_name: "sam",
        paths: &w.paths,
        launch: &w.launch,
        global_env: &w.global_env,
        host_env: &w.host_env,
        user_dirs: &w.user_dirs,
        other_game_roots: &w.other_roots,
        mounts: &w.mounts,
        fs: &w.fs,
        developer_mode: w.developer_mode,
        seccomp_available: w.seccomp_available,
        isolation: w.runtime.isolation,
    }
}

fn main() {
    let mut reports = Vec::new();
    let w = SandboxWorld::worked_example();
    let policy = offline();
    reports.push(
        Bench::new("plan_offline", Duration::from_millis(2))
            .iterations(200)
            .run(|| render(&plan(&policy, &w.ctx(), &w.host).unwrap()).unwrap()),
    );
    let grants = many_grants(&w);
    reports.push(
        Bench::new("plan_many_grants", Duration::from_millis(2))
            .iterations(200)
            .run(|| render(&plan(&grants, &w.ctx(), &w.host).unwrap()).unwrap()),
    );
    reports.push(
        Bench::new("policy_explain", Duration::from_micros(500))
            .iterations(200)
            .run(|| explain_policy(&grants, &pctx(&w), &w.host)),
    );

    // Quick hash: two 1 MiB windows of a 64 MiB file on the target directory's disk.
    let dir = tempfile::Builder::new()
        .prefix("bench-planning-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .unwrap();
    let exe = dir.path().join("game.exe");
    let mut big = sample_game();
    big.resize(64 << 20, 0x5a);
    let fs = Arc::new(RealFs::new());
    fs.write_new(&exe, &big, 0o600).unwrap();
    reports.push(
        Bench::new("fingerprint_quick_hash", Duration::from_millis(15))
            .iterations(50)
            .run(|| fingerprint::compute(&*fs, &exe).unwrap()),
    );

    let pe = sample_game();
    let limits = PeLimits::default();
    reports.push(
        Bench::new("pe_inspect_with_icon", Duration::from_millis(50))
            .iterations(100)
            .run(|| {
                (
                    inspect_bytes(&pe, &limits).unwrap(),
                    largest_icon_bytes(&pe, &limits).unwrap(),
                )
            }),
    );

    // Thumbnail of a 1200 × 1800 cover: scale to 480 × 720 and encode (per size).
    let original = RgbaImage {
        width: 1200,
        height: 1800,
        pixels: (0..1200u32 * 1800)
            .flat_map(|i| {
                let (x, y) = (i % 1200, i / 1200);
                [(x / 5) as u8, (y / 7) as u8, ((x ^ y) & 0xff) as u8, 255]
            })
            .collect(),
    };
    let codec = ImageCrateCodec;
    reports.push(
        Bench::new("thumbnail_cover_480", Duration::from_millis(60))
            .iterations(20)
            .run(|| {
                codec
                    .encode_png(&scale_to(&original, ThumbSize::Cover480))
                    .unwrap()
            }),
    );
    finish(&reports);
}
