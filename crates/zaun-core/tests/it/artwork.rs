//! Artwork: codec limits, import round trips, thumbnails, store maintenance, the Steam ID
//! chain and the allow-listed HTTP client against a local fake server (ART-001…ART-008).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use zaun_core::artwork::codec::{CodecError, ImageCodec, ImageCrateCodec, ImageLimits, RgbaImage};
use zaun_core::artwork::http::{
    HopLimits, HttpClient, HttpError, HttpsUrl, MAX_BODY, RawResponse, Transport, UreqTransport,
};
use zaun_core::artwork::import::{ImportDeps, ImportInput, import};
use zaun_core::artwork::steam::{
    self, AttemptTracker, FetchOutcome, Resolution, ResolveDeps, SlotOutcome, SteamAppIdSource,
};
use zaun_core::artwork::store::{self, SweepOptions};
use zaun_core::artwork::thumbs::{self, ArtIndex, ThumbDeps, ThumbResult, ThumbService};
use zaun_core::artwork::{ThumbKey, ThumbSize};
use zaun_core::clock::{SystemClock, Timestamp};
use zaun_core::config::schema::{ArtworkOnline, ArtworkPrefs};
use zaun_core::error::ErrorCode;
use zaun_core::fsx::Fs;
use zaun_core::jobs::{JobPool, JobPoolConfig, JobPriority};
use zaun_core::model::{ArtOrigin, ArtSlot, Game};
use zaun_core::paths::{AppPaths, PathEnv};
use zaun_core::storage::{ArtworkRepo, Db, GameRepo};
use zaun_testkit::fixtures::games::{self, BASE_MS};
use zaun_testkit::fixtures::images;
use zaun_testkit::{FakeClock, FakeHttpServer, MemFs, Reply};

const C: ImageCrateCodec = ImageCrateCodec;

fn png(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    C.encode_png(&RgbaImage::filled(w, h, rgba)).unwrap()
}

fn jpeg(w: u32, h: u32) -> Vec<u8> {
    C.encode_jpeg(&RgbaImage::filled(w, h, [200, 100, 50, 255]), 80)
        .unwrap()
}

fn paths() -> AppPaths {
    AppPaths::resolve(
        &PathEnv::new(1000)
            .with("HOME", "/home/sam")
            .with("XDG_RUNTIME_DIR", "/run/user/1000"),
    )
    .unwrap()
}

fn now() -> Timestamp {
    Timestamp::from_millis(BASE_MS)
}

fn steam_on() -> ArtworkPrefs {
    ArtworkPrefs {
        online: ArtworkOnline::Steam,
        name_search: true,
    }
}

fn steam_off() -> ArtworkPrefs {
    ArtworkPrefs {
        online: ArtworkOnline::Off,
        name_search: true,
    }
}

// ---- ART-001: codec limits and bombs --------------------------------------------------------

/// Acceptance (ART-001): decompression-bomb samples are rejected before allocation. Each
/// file is a few dozen bytes claiming gigapixels; the probe refuses it from the header, so
/// the test also completes instantly and without memory growth.
#[test]
fn decompression_bombs_are_rejected_from_the_header() {
    let l = ImageLimits::DEFAULT;
    let bombs: Vec<(&str, Vec<u8>)> = vec![
        ("png 100k²", images::png_bomb(100_000, 100_000)),
        (
            "png 16k² (side ok, 256 MP)",
            images::png_bomb(16_000, 16_000),
        ),
        ("png 1×2^31", images::png_bomb(1, 0x7FFF_FFFF)),
        ("jpeg 65535²", images::jpeg_bomb(65_535, 65_535)),
        ("jpeg 10000²", images::jpeg_bomb(10_000, 10_000)),
        ("bmp 50000²", images::bmp_bomb(50_000, 50_000)),
        ("bmp top-down 50000²", images::bmp_bomb(50_000, -50_000)),
        ("webp 16M²", images::webp_bomb(16_000_000, 16_000_000)),
        ("webp 9000²", images::webp_bomb(9_000, 9_000)),
        (
            "ico with png bomb",
            images::ico_with_png_bomb(100_000, 100_000),
        ),
    ];
    for (name, b) in bombs {
        assert!(b.len() < 512, "{name}: fixture should be tiny");
        let r = C.probe(&b, &l);
        assert!(
            matches!(
                r,
                Err(CodecError::TooLarge(_)) | Err(CodecError::Malformed(_))
            ),
            "{name}: probe {r:?}"
        );
        let r = C.decode(&b, &l);
        assert!(r.is_err(), "{name}: decode {r:?}");
    }
    // A real, highly compressible image over the pixel limit: refused before decoding.
    let big = png(2000, 2000, [0, 0, 0, 255]);
    assert!(big.len() < 200_000);
    let small_limits = ImageLimits {
        max_pixels: 1_000_000,
        ..ImageLimits::DEFAULT
    };
    assert!(matches!(
        C.decode(&big, &small_limits),
        Err(CodecError::TooLarge(_))
    ));
}

#[test]
fn malformed_corpus_never_panics_and_is_refused() {
    for (name, b) in images::malformed_corpus() {
        let r = C.decode(&b, &ImageLimits::DEFAULT);
        assert!(r.is_err(), "{name}: {r:?}");
        assert!(
            !matches!(r, Err(CodecError::Panicked)),
            "{name}: decoder panicked"
        );
    }
}

#[test]
fn every_accepted_format_decodes() {
    let base = RgbaImage::filled(6, 4, [1, 2, 3, 255]);
    let mut samples = vec![("png", C.encode_png(&base).unwrap()), ("jpeg", jpeg(6, 4))];
    for (name, fmt) in [
        ("bmp", image_fmt::Bmp),
        ("ico", image_fmt::Ico),
        ("webp", image_fmt::WebP),
    ] {
        samples.push((name, image_fmt::encode(&base, fmt)));
    }
    for (name, b) in samples {
        let img = C
            .decode(&b, &ImageLimits::DEFAULT)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((img.width, img.height), (6, 4), "{name}");
    }
}

/// Encoders for formats the production codec only reads.
mod image_fmt {
    use zaun_core::artwork::codec::RgbaImage;

    pub enum Fmt {
        Bmp,
        Ico,
        WebP,
    }
    pub use Fmt::*;

    pub fn encode(img: &RgbaImage, f: Fmt) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let buf = image::RgbaImage::from_raw(img.width, img.height, img.pixels.clone()).unwrap();
        let fmt = match f {
            Bmp => image::ImageFormat::Bmp,
            Ico => image::ImageFormat::Ico,
            WebP => image::ImageFormat::WebP,
        };
        buf.write_to(&mut out, fmt).unwrap();
        out.into_inner()
    }
}

// ---- ART-002: import round trip --------------------------------------------------------------

struct Fixture {
    fs: Arc<MemFs>,
    paths: AppPaths,
    db: Db,
    game: Game,
}

fn fixture() -> Fixture {
    let mut db = Db::open_in_memory().unwrap();
    let game = games::game(1, 1);
    db.insert_game(&game).unwrap();
    Fixture {
        fs: Arc::new(MemFs::new()),
        paths: paths(),
        db,
        game,
    }
}

fn deps_of<'a>(fs: &'a MemFs, paths: &'a AppPaths) -> ImportDeps<'a> {
    ImportDeps {
        fs,
        codec: &C,
        paths,
        limits: ImageLimits::DEFAULT,
    }
}

impl Fixture {
    fn files(&self) -> Vec<String> {
        self.fs
            .read_dir(&self.paths.artwork_game_dir(self.game.id))
            .map(|v| {
                v.into_iter()
                    .map(|e| e.name.to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[test]
fn import_round_trip_replace_and_idempotence() {
    let mut f = fixture();
    f.fs.file(
        "/home/sam/Pictures/a.png",
        png(1920, 1080, [10, 200, 30, 255]),
    );
    // A user-chosen file may be a symlink: it is followed.
    f.fs.link("/home/sam/Pictures/link.png", "/home/sam/Pictures/a.png");
    let deps = deps_of(&f.fs, &f.paths);
    let gid = f.game.id;
    let a = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Cover,
        ImportInput::File(Path::new("/home/sam/Pictures/link.png")),
        now(),
    )
    .unwrap();
    assert_eq!((a.art.width, a.art.height), (720, 1080));
    assert_eq!(a.art.source, ArtOrigin::File);
    assert_eq!(a.dominant, [10, 200, 30]);
    assert!(a.replaced.is_none());
    assert!(a.art.file.starts_with("cover-") && a.art.file.ends_with(".png"));
    let stored =
        f.fs.read(&store::original_path(&f.paths, &a.art), u64::MAX)
            .unwrap();
    assert_eq!(
        zaun_core::artwork::sniff(&stored),
        Some(zaun_core::artwork::ImageFormat::Png)
    );
    assert_eq!(f.db.artwork_of(gid).unwrap(), vec![a.art.clone()]);

    // Same picture again: same name, nothing left over.
    let again = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Cover,
        ImportInput::File(Path::new("/home/sam/Pictures/a.png")),
        now(),
    )
    .unwrap();
    assert_eq!(again.art.file, a.art.file);
    assert_eq!(f.files(), vec![a.art.file.clone()]);

    // A different picture replaces the row and removes the old file.
    let b = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Cover,
        ImportInput::Bytes {
            bytes: &jpeg(600, 900),
            origin: ArtOrigin::SteamCdn,
        },
        now(),
    )
    .unwrap();
    assert_eq!(
        b.replaced.as_ref().map(|r| r.file.clone()),
        Some(a.art.file.clone())
    );
    assert_eq!(f.files(), vec![b.art.file.clone()]);
    assert_eq!(f.db.artwork_of(gid).unwrap()[0].source, ArtOrigin::SteamCdn);

    // Hero is JPEG, icon is padded square PNG.
    let h = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Hero,
        ImportInput::Bytes {
            bytes: &jpeg(1920, 620),
            origin: ArtOrigin::SteamCdn,
        },
        now(),
    )
    .unwrap();
    assert!(h.art.file.ends_with(".jpg"));
    assert_eq!((h.art.width, h.art.height), (1920, 600));
    let i = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Icon,
        ImportInput::Bytes {
            bytes: &png(48, 32, [1, 1, 1, 255]),
            origin: ArtOrigin::ExecutableIcon,
        },
        now(),
    )
    .unwrap();
    assert_eq!((i.art.width, i.art.height), (48, 48));
    assert_eq!(f.files().len(), 3);
}

#[test]
fn failed_imports_keep_the_previous_artwork() {
    let mut f = fixture();
    let deps = deps_of(&f.fs, &f.paths);
    let gid = f.game.id;
    let a = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Cover,
        ImportInput::Bytes {
            bytes: &png(60, 90, [5, 5, 5, 255]),
            origin: ArtOrigin::File,
        },
        now(),
    )
    .unwrap();
    let html = b"<html>not an image</html>";
    let e = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Cover,
        ImportInput::Bytes {
            bytes: html,
            origin: ArtOrigin::SteamCdn,
        },
        now(),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::ArtUnsupportedFormat);
    let small = ImportDeps {
        limits: ImageLimits {
            max_bytes: 100,
            ..ImageLimits::DEFAULT
        },
        ..deps
    };
    f.fs.file("/home/sam/big.png", png(300, 300, [9, 9, 9, 255]));
    let e = import(
        &small,
        &mut f.db,
        gid,
        ArtSlot::Cover,
        ImportInput::File(Path::new("/home/sam/big.png")),
        now(),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::ArtTooLarge);
    let bomb = images::png_bomb(50_000, 50_000);
    let e = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Cover,
        ImportInput::Bytes {
            bytes: &bomb,
            origin: ArtOrigin::File,
        },
        now(),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::ArtTooLarge);
    assert_eq!(f.db.artwork_of(gid).unwrap(), vec![a.art.clone()]);
    assert_eq!(f.files(), vec![a.art.file]);
}

// ---- ART-004: thumbnails -----------------------------------------------------------------------

fn pool() -> Arc<JobPool> {
    Arc::new(
        JobPool::new(
            JobPoolConfig {
                workers: 2,
                ..JobPoolConfig::default()
            },
            Arc::new(SystemClock::new()),
        )
        .unwrap(),
    )
}

fn wait_result(s: &ThumbService) -> Option<ThumbResult> {
    for _ in 0..500 {
        if let Some(r) = s.try_recv() {
            return Some(r);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

#[test]
fn thumbnails_generate_lazily_then_load_from_disk() {
    let mut f = fixture();
    let deps = deps_of(&f.fs, &f.paths);
    let gid = f.game.id;
    let a = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Cover,
        ImportInput::Bytes {
            bytes: &png(1200, 1800, [40, 80, 120, 255]),
            origin: ArtOrigin::File,
        },
        now(),
    )
    .unwrap();
    let index = Arc::new(ArtIndex::new());
    index.replace_all(&f.paths, &f.db.artwork_of(gid).unwrap());
    let wakes = Arc::new(AtomicUsize::new(0));
    let w = wakes.clone();
    let svc = ThumbService::new(ThumbDeps {
        pool: pool(),
        fs: f.fs.clone(),
        codec: Arc::new(C),
        thumbs_dir: f.paths.thumbs_dir.clone(),
        index,
        wake: Arc::new(move || {
            w.fetch_add(1, Ordering::Relaxed);
        }),
    });
    let key = ThumbKey {
        art_hash: a.art.hash,
        size: ThumbSize::Cover240,
    };
    svc.request(key, JobPriority::Interactive);
    svc.request(key, JobPriority::Interactive); // de-duplicated
    let Some(ThumbResult::Ready(p)) = wait_result(&svc) else {
        panic!("no thumbnail")
    };
    assert_eq!((p.image.width, p.image.height), (240, 360));
    assert_eq!(p.dominant, [40, 80, 120]);
    assert_eq!(wakes.load(Ordering::Relaxed), 1);
    assert!(svc.try_recv().is_none());
    // Both cover sizes were written, with the colour chunk.
    for size in ThumbSize::for_slot(ArtSlot::Cover) {
        let path = thumbs::thumb_path(
            &f.paths.thumbs_dir,
            &ThumbKey {
                art_hash: a.art.hash,
                size: *size,
            },
        );
        let bytes = f.fs.read(&path, u64::MAX).unwrap();
        assert_eq!(thumbs::read_colour_chunk(&bytes), Some([40, 80, 120]));
    }
    // Second request is served from disk even with the original gone.
    f.fs.remove_file(&store::original_path(&f.paths, &a.art))
        .unwrap();
    let key480 = ThumbKey {
        art_hash: a.art.hash,
        size: ThumbSize::Cover480,
    };
    svc.request(key480, JobPriority::Normal);
    let Some(ThumbResult::Ready(p)) = wait_result(&svc) else {
        panic!("no thumbnail")
    };
    assert_eq!((p.image.width, p.image.height), (480, 720));

    // Unknown hash → Failed, placeholder stays.
    let unknown = ThumbKey {
        art_hash: [7; 32],
        size: ThumbSize::Cover240,
    };
    svc.request(unknown, JobPriority::Interactive);
    assert!(matches!(
        wait_result(&svc),
        Some(ThumbResult::Failed { .. })
    ));
    assert_eq!(svc.in_flight(), 0);
}

#[test]
fn cancelled_thumbnail_requests_yield_nothing() {
    let f = fixture();
    // A pool with one worker blocked on a gate, so requests stay queued.
    let pool = pool();
    let gate = Arc::new(parking_lot::Mutex::new(()));
    let held = gate.lock();
    for _ in 0..2 {
        let g = gate.clone();
        let _ = pool.submit(
            JobPriority::Interactive,
            zaun_core::jobs::JobLabel("block"),
            move |_| {
                drop(g.lock());
            },
        );
    }
    let svc = ThumbService::new(ThumbDeps {
        pool,
        fs: f.fs.clone(),
        codec: Arc::new(C),
        thumbs_dir: f.paths.thumbs_dir.clone(),
        index: Arc::new(ArtIndex::new()),
        wake: Arc::new(|| {}),
    });
    let key = ThumbKey {
        art_hash: [1; 32],
        size: ThumbSize::Icon64,
    };
    svc.request(key, JobPriority::Interactive);
    svc.cancel(key);
    assert_eq!(svc.in_flight(), 0);
    drop(held);
    std::thread::sleep(Duration::from_millis(100));
    assert!(svc.try_recv().is_none());
}

// ---- ART-006: store maintenance ----------------------------------------------------------------

#[test]
fn sweep_never_deletes_referenced_files_and_trims_thumbnails() {
    let mut f = fixture();
    let deps = deps_of(&f.fs, &f.paths);
    let gid = f.game.id;
    let a = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Cover,
        ImportInput::Bytes {
            bytes: &png(60, 90, [1, 2, 3, 255]),
            origin: ArtOrigin::File,
        },
        now(),
    )
    .unwrap();
    let dir = f.paths.artwork_game_dir(gid);
    let orphan_dir = f
        .paths
        .artwork_dir
        .join("0190d1e0-0000-7000-8000-00000000dead");
    let old = 1_000_000_000_000_000_000i64;
    f.fs.set_now(old);
    f.fs.file(dir.join("icon-00000000000000aa.png"), b"orphan");
    f.fs.file(
        orphan_dir.join("cover-00000000000000bb.png"),
        b"orphan of a removed game",
    );
    f.fs.file(dir.join("notes.txt"), b"not ours");
    f.fs.link(dir.join("hero-00000000000000cc.jpg"), "/etc/passwd");
    let t = |h: &str, s: &str| f.paths.thumbs_dir.join(format!("{h}-{s}.png"));
    let mine = store::hash16(&a.art.hash);
    f.fs.file(t(&mine, "240x360"), vec![0u8; 400]);
    f.fs.set_now(old + 10);
    f.fs.file(t(&mine, "480x720"), vec![0u8; 600]);
    f.fs.file(t("00000000000000dd", "240x360"), vec![0u8; 100]);
    // Young orphan (an import in progress): kept.
    let now_ns = old + 2 * 3_600_000_000_000;
    f.fs.set_now(now_ns - 1_000_000_000);
    f.fs.file(dir.join("cover-00000000000000ee.png"), b"young");
    // The referenced original is old too.
    f.fs.set_mtime(store::original_path(&f.paths, &a.art), old);

    let referenced = f.db.referenced_artwork_hashes().unwrap();
    let r = store::sweep(
        &*f.fs,
        &f.paths,
        &referenced,
        SweepOptions {
            now_ns,
            thumb_cap: 700,
        },
    );
    assert_eq!(r.originals_removed, 2, "{r:?}");
    assert!(f.fs.exists(&store::original_path(&f.paths, &a.art)));
    assert!(f.fs.exists(&dir.join("notes.txt")));
    assert!(
        f.fs.exists(&dir.join("hero-00000000000000cc.jpg")),
        "symlinks are not ours"
    );
    assert!(f.fs.exists(&dir.join("cover-00000000000000ee.png")));
    assert!(!f.fs.exists(&orphan_dir), "empty game directory removed");
    // Thumbnails: the orphan goes, then the oldest referenced one to get under 700 bytes.
    assert_eq!(r.thumbs_removed, 2);
    assert!(!f.fs.exists(&t(&mine, "240x360")));
    assert!(f.fs.exists(&t(&mine, "480x720")));
    assert_eq!(r.errors, 0);
}

#[test]
fn broken_rows_are_repaired() {
    let mut f = fixture();
    let deps = deps_of(&f.fs, &f.paths);
    let gid = f.game.id;
    let a = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Cover,
        ImportInput::Bytes {
            bytes: &png(60, 90, [1, 2, 3, 255]),
            origin: ArtOrigin::File,
        },
        now(),
    )
    .unwrap();
    let i = import(
        &deps,
        &mut f.db,
        gid,
        ArtSlot::Icon,
        ImportInput::Bytes {
            bytes: &png(16, 16, [1, 2, 3, 255]),
            origin: ArtOrigin::File,
        },
        now(),
    )
    .unwrap();
    f.fs.remove_file(&store::original_path(&f.paths, &a.art))
        .unwrap();
    let fixed = store::repair_broken_rows(&*f.fs, &f.paths, &mut f.db, &[gid]).unwrap();
    assert_eq!(fixed, vec![(gid, ArtSlot::Cover)]);
    assert_eq!(f.db.artwork_of(gid).unwrap(), vec![i.art]);
}

// ---- ART-007: ID chain -------------------------------------------------------------------------

/// A transport that must never be used.
struct Panics;
impl Transport for Panics {
    fn get(&self, url: &HttpsUrl, _: HopLimits) -> Result<RawResponse, HttpError> {
        panic!("network used while offline: {url}");
    }
}

/// Canned responses by URL; counts calls.
#[derive(Default)]
struct Canned {
    map: Vec<(String, RawResponse)>,
    calls: AtomicUsize,
}
impl Canned {
    fn with(mut self, url: &str, status: u16, body: &[u8]) -> Self {
        self.map.push((
            url.into(),
            RawResponse {
                status,
                location: None,
                body: body.to_vec(),
            },
        ));
        self
    }
}
impl Transport for Canned {
    fn get(&self, url: &HttpsUrl, _: HopLimits) -> Result<RawResponse, HttpError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let u = url.to_string();
        Ok(self
            .map
            .iter()
            .find(|(k, _)| *k == u)
            .map(|(_, r)| r.clone())
            .unwrap_or(RawResponse {
                status: 404,
                location: None,
                body: Vec::new(),
            }))
    }
}

fn client(t: impl Transport + 'static) -> HttpClient {
    HttpClient::new(Arc::new(t), Arc::new(SystemClock::new()))
}

fn game_named(name: &str, root: Option<&str>) -> Game {
    let mut g = games::game(2, 1);
    g.name = name.into();
    g.root = root.map(PathBuf::from);
    g
}

#[test]
fn id_chain_table() {
    let file = |p: &Path| (p == Path::new("/games/hl")).then_some(70u32);
    let umu = |g: &Game| (g.name == "Umu Game").then_some(1_145_360u32);
    let search_one = br#"{"total":2,"items":[{"type":"app","name":"Hades II","id":1145350},{"type":"app","name":"Hades","id":1145360}]}"#;
    let search_many = br#"{"total":2,"items":[{"type":"app","name":"Doom","id":1},{"type":"app","name":"DOOM","id":2}]}"#;
    let su = |n: &str| steam::search_url(n);
    let canned = Canned::default()
        .with(&su("Hades"), 200, search_one)
        .with(&su("Doom"), 200, search_many)
        .with(&su("Broken"), 200, b"<html>")
        .with(&su("Nothing"), 200, br#"{"total":0,"items":[]}"#);
    let http = client(canned);
    let deps = ResolveDeps {
        app_id_file: &file,
        umu_db: &umu,
        http: Some(&http),
    };
    let mut stored = game_named("Whatever", Some("/games/hl"));
    stored.steam_app_id = Some(2_246_340);
    let found = |id, source| Resolution::Found { app_id: id, source };
    let rows: Vec<(Game, ArtworkPrefs, Resolution)> = vec![
        (
            stored.clone(),
            steam_off(),
            found(2_246_340, SteamAppIdSource::Stored),
        ),
        (
            game_named("Whatever", Some("/games/hl")),
            steam_off(),
            found(70, SteamAppIdSource::AppIdFile),
        ),
        (
            game_named("Umu Game", None),
            steam_off(),
            found(1_145_360, SteamAppIdSource::UmuDb),
        ),
        (game_named("Hades", None), steam_off(), Resolution::NotFound),
        (
            game_named("Hades", None),
            ArtworkPrefs {
                name_search: false,
                ..steam_on()
            },
            Resolution::NotFound,
        ),
        (
            game_named("Hades", None),
            steam_on(),
            found(1_145_360, SteamAppIdSource::NameSearch),
        ),
        (
            game_named("Nothing", None),
            steam_on(),
            Resolution::NotFound,
        ),
        (game_named("Broken", None), steam_on(), Resolution::NotFound),
        (
            game_named("Unknown", None),
            steam_on(),
            Resolution::NotFound,
        ),
    ];
    for (g, prefs, want) in rows {
        assert_eq!(
            steam::resolve_app_id(&prefs, &g, &deps),
            want,
            "{} {:?}",
            g.name,
            prefs.online
        );
    }
    let Resolution::Candidates(c) =
        steam::resolve_app_id(&steam_on(), &game_named("Doom", None), &deps)
    else {
        panic!("candidates expected");
    };
    assert_eq!(c.len(), 2);
}

#[test]
fn user_set_ids_are_never_replaced() {
    let mut g = game_named("X", None);
    assert!(steam::apply_app_id(
        &mut g,
        70,
        SteamAppIdSource::NameSearch
    ));
    assert_eq!(
        (g.steam_app_id, g.steam_app_id_is_user_set),
        (Some(70), false)
    );
    assert!(steam::apply_app_id(&mut g, 220, SteamAppIdSource::User));
    assert_eq!(
        (g.steam_app_id, g.steam_app_id_is_user_set),
        (Some(220), true)
    );
    for src in [
        SteamAppIdSource::AppIdFile,
        SteamAppIdSource::UmuDb,
        SteamAppIdSource::NameSearch,
        SteamAppIdSource::Stored,
    ] {
        assert!(!steam::apply_app_id(&mut g, 70, src));
        assert_eq!(g.steam_app_id, Some(220));
    }
}

/// Acceptance (ART-007): with `online = "off"` nothing is sent.
#[test]
fn offline_never_touches_the_client() {
    let mut f = fixture();
    let http = client(Panics);
    let none = |_: &Path| None;
    let none_g = |_: &Game| None;
    let deps = ResolveDeps {
        app_id_file: &none,
        umu_db: &none_g,
        http: Some(&http),
    };
    assert_eq!(
        steam::resolve_app_id(&steam_off(), &game_named("Hades", None), &deps),
        Resolution::NotFound
    );
    assert_eq!(
        steam::fetch_slot(&steam_off(), &http, &C, 70, ArtSlot::Cover),
        FetchOutcome::Offline
    );
    let tracker = AttemptTracker::new();
    let gid = f.game.id;
    let imp = deps_of(&f.fs, &f.paths);
    let r = steam::fetch_and_import(
        &steam_off(),
        &http,
        &imp,
        &mut f.db,
        &tracker,
        gid,
        70,
        ArtSlot::Cover,
        true,
        now(),
    )
    .unwrap();
    assert_eq!(r, SlotOutcome::Skipped);
    assert!(!tracker.was_tried(gid, ArtSlot::Cover));
}

#[test]
fn concurrency_is_capped_at_two() {
    struct Slow {
        now: AtomicUsize,
        max: AtomicUsize,
    }
    impl Transport for Slow {
        fn get(&self, _: &HttpsUrl, _: HopLimits) -> Result<RawResponse, HttpError> {
            let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
            self.max.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(30));
            self.now.fetch_sub(1, Ordering::SeqCst);
            Ok(RawResponse {
                status: 404,
                location: None,
                body: Vec::new(),
            })
        }
    }
    let t = Arc::new(Slow {
        now: AtomicUsize::new(0),
        max: AtomicUsize::new(0),
    });
    let http = Arc::new(HttpClient::new(t.clone(), Arc::new(SystemClock::new())));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let h = http.clone();
            std::thread::spawn(move || {
                h.get(&format!(
                    "https://cdn.cloudflare.steamstatic.com/steam/apps/{i}/x.jpg"
                ))
                .unwrap()
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(t.max.load(Ordering::SeqCst), 2);
}

#[test]
fn first_requests_only_to_request_hosts() {
    let http = client(Panics);
    for url in [
        "https://shared.steamstatic.com/x.jpg",
        "https://evil.example/x.jpg",
        "https://cdn.cloudflare.steamstatic.com.evil.example/x.jpg",
    ] {
        assert!(
            matches!(http.get(url), Err(HttpError::HostNotAllowed(_))),
            "{url}"
        );
    }
    assert_eq!(
        http.get("http://cdn.cloudflare.steamstatic.com/x")
            .unwrap_err(),
        HttpError::NotHttps
    );
}

// ---- Real client against the local fake server -------------------------------------------------

const CDN: &str = "/cdn.cloudflare.steamstatic.com/steam/apps";

fn loopback(server: &FakeHttpServer) -> HttpClient {
    HttpClient::new(
        Arc::new(UreqTransport::plaintext_loopback(server.addr())),
        Arc::new(SystemClock::new()),
    )
}

#[test]
fn fake_server_redirects_within_and_outside_the_allow_list() {
    let s = FakeHttpServer::start().unwrap();
    let img = png(6, 9, [1, 2, 3, 255]);
    s.route(
        &format!("{CDN}/1/a.jpg"),
        Reply::redirect("https://shared.steamstatic.com/store_item_assets/1/a.jpg"),
    )
    .route(
        "/shared.steamstatic.com/store_item_assets/1/a.jpg",
        Reply::ok(img.clone()),
    )
    .route(
        &format!("{CDN}/2/a.jpg"),
        Reply::redirect("https://evil.example/a.jpg"),
    )
    .route(
        &format!("{CDN}/3/a.jpg"),
        Reply::redirect("http://shared.steamstatic.com/a.jpg"),
    )
    // A relative redirect to itself: a loop.
    .route(
        &format!("{CDN}/4/a.jpg"),
        Reply::redirect("/steam/apps/4/a.jpg"),
    )
    .route(
        &format!("{CDN}/5/a.jpg"),
        Reply::redirect("https://127.0.0.1/a.jpg"),
    );
    let http = loopback(&s);
    let r = http
        .get("https://cdn.cloudflare.steamstatic.com/steam/apps/1/a.jpg")
        .unwrap();
    assert_eq!((r.status, r.body), (200, img));
    assert_eq!(r.url.host, "shared.steamstatic.com");
    assert_eq!(
        http.get("https://cdn.cloudflare.steamstatic.com/steam/apps/2/a.jpg")
            .unwrap_err(),
        HttpError::HostNotAllowed("evil.example".into())
    );
    assert!(
        !s.requests().iter().any(|(p, _)| p.contains("evil.example")),
        "never contacted"
    );
    assert_eq!(
        http.get("https://cdn.cloudflare.steamstatic.com/steam/apps/3/a.jpg")
            .unwrap_err(),
        HttpError::NotHttps
    );
    assert_eq!(
        http.get("https://cdn.cloudflare.steamstatic.com/steam/apps/4/a.jpg")
            .unwrap_err(),
        HttpError::Redirects
    );
    let loops = s
        .requests()
        .iter()
        .filter(|(p, _)| p.ends_with("/4/a.jpg"))
        .count();
    assert_eq!(loops, zaun_core::artwork::http::MAX_REDIRECTS + 1);
    assert_eq!(
        http.get("https://cdn.cloudflare.steamstatic.com/steam/apps/5/a.jpg")
            .unwrap_err(),
        HttpError::BadUrl
    );
    // User-Agent on every request.
    let ua = format!("Zaun/{}", zaun_core::consts::APP_VERSION);
    assert!(
        s.requests()
            .iter()
            .all(|(_, a)| a.as_deref() == Some(ua.as_str()))
    );
}

#[test]
fn fake_server_oversize_bodies_are_cut_off() {
    let s = FakeHttpServer::start().unwrap();
    let over = MAX_BODY as usize + 1;
    s.route(&format!("{CDN}/1/big.jpg"), Reply::ok(vec![0u8; over]))
        .route(&format!("{CDN}/2/big.jpg"), Reply::Chunked { len: over })
        .route(&format!("{CDN}/3/ok.jpg"), Reply::Chunked { len: 1000 });
    let http = loopback(&s);
    assert_eq!(
        http.get("https://cdn.cloudflare.steamstatic.com/steam/apps/1/big.jpg")
            .unwrap_err(),
        HttpError::TooLarge
    );
    assert_eq!(
        http.get("https://cdn.cloudflare.steamstatic.com/steam/apps/2/big.jpg")
            .unwrap_err(),
        HttpError::TooLarge
    );
    assert_eq!(
        http.get("https://cdn.cloudflare.steamstatic.com/steam/apps/3/ok.jpg")
            .unwrap()
            .body
            .len(),
        1000
    );
    // A smaller cap (store search) applies as well.
    assert_eq!(
        http.get_capped(
            "https://cdn.cloudflare.steamstatic.com/steam/apps/3/ok.jpg",
            999
        )
        .unwrap_err(),
        HttpError::TooLarge
    );
}

#[test]
fn fake_server_timeout() {
    let s = FakeHttpServer::start().unwrap();
    s.route(
        &format!("{CDN}/1/slow.jpg"),
        Reply::Stall {
            hold: Duration::from_secs(3),
        },
    );
    let http = loopback(&s).with_timeout(Duration::from_millis(300));
    assert_eq!(
        http.get("https://cdn.cloudflare.steamstatic.com/steam/apps/1/slow.jpg")
            .unwrap_err(),
        HttpError::Timeout
    );
}

#[test]
fn fake_server_fallback_matrix() {
    let s = FakeHttpServer::start().unwrap();
    let cover = jpeg(600, 900);
    // 10: 2x → 404, 1x → image.  11: everything 404.  12: 2x is HTML, 1x 404.
    // 13: 2x → 500 (stop).  14: hero 404 then header.jpg.
    s.route(
        &format!("{CDN}/10/library_600x900.jpg"),
        Reply::ok(cover.clone()),
    )
    .route(
        &format!("{CDN}/12/library_600x900_2x.jpg"),
        Reply::ok(b"<html>Sorry</html>".to_vec()),
    )
    .route(
        &format!("{CDN}/13/library_600x900_2x.jpg"),
        Reply::status(500),
    )
    .route(&format!("{CDN}/14/header.jpg"), Reply::ok(jpeg(460, 215)));
    let http = loopback(&s);
    let on = steam_on();
    match steam::fetch_slot(&on, &http, &C, 10, ArtSlot::Cover) {
        FetchOutcome::Fetched { bytes, url } => {
            assert_eq!(bytes, cover);
            assert!(url.ends_with("/10/library_600x900.jpg"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        steam::fetch_slot(&on, &http, &C, 11, ArtSlot::Cover),
        FetchOutcome::NotAvailable
    );
    assert_eq!(
        steam::fetch_slot(&on, &http, &C, 12, ArtSlot::Cover),
        FetchOutcome::NotAvailable
    );
    assert!(matches!(
        steam::fetch_slot(&on, &http, &C, 13, ArtSlot::Cover),
        FetchOutcome::Failed(_)
    ));
    assert!(matches!(
        steam::fetch_slot(&on, &http, &C, 14, ArtSlot::Hero),
        FetchOutcome::Fetched { .. }
    ));

    // Import path: steam_cdn row, one attempt per slot, a 404 slot gives no error.
    let mut f = fixture();
    let gid = f.game.id;
    let imp = deps_of(&f.fs, &f.paths);
    let tracker = AttemptTracker::new();
    let r = steam::fetch_and_import(
        &on,
        &http,
        &imp,
        &mut f.db,
        &tracker,
        gid,
        10,
        ArtSlot::Cover,
        false,
        now(),
    )
    .unwrap();
    let SlotOutcome::Imported(i) = r else {
        panic!("{r:?}")
    };
    assert_eq!(i.art.source, ArtOrigin::SteamCdn);
    assert_eq!((i.art.width, i.art.height), (600, 900));
    let before = s.requests().len();
    // Never again automatically, even with a fresh tracker (the row is the record).
    let r = steam::fetch_and_import(
        &on,
        &http,
        &imp,
        &mut f.db,
        &AttemptTracker::new(),
        gid,
        10,
        ArtSlot::Cover,
        false,
        now(),
    )
    .unwrap();
    assert_eq!(r, SlotOutcome::Skipped);
    assert_eq!(s.requests().len(), before);
    // A 404 hero: NotAvailable once, then skipped.
    let r = steam::fetch_and_import(
        &on,
        &http,
        &imp,
        &mut f.db,
        &tracker,
        gid,
        11,
        ArtSlot::Hero,
        false,
        now(),
    )
    .unwrap();
    assert_eq!(r, SlotOutcome::NotAvailable);
    let r = steam::fetch_and_import(
        &on,
        &http,
        &imp,
        &mut f.db,
        &tracker,
        gid,
        11,
        ArtSlot::Hero,
        false,
        now(),
    )
    .unwrap();
    assert_eq!(r, SlotOutcome::Skipped);
    // The user asks again: tried again.
    let r = steam::fetch_and_import(
        &on,
        &http,
        &imp,
        &mut f.db,
        &tracker,
        gid,
        14,
        ArtSlot::Hero,
        true,
        now(),
    )
    .unwrap();
    assert!(matches!(r, SlotOutcome::Imported(_)));
    // A user file always wins.
    f.fs.file("/home/sam/mine.png", png(60, 90, [0, 0, 0, 255]));
    import(
        &imp,
        &mut f.db,
        gid,
        ArtSlot::Cover,
        ImportInput::File(Path::new("/home/sam/mine.png")),
        now(),
    )
    .unwrap();
    let r = steam::fetch_and_import(
        &on,
        &http,
        &imp,
        &mut f.db,
        &tracker,
        gid,
        10,
        ArtSlot::Cover,
        true,
        now(),
    )
    .unwrap();
    assert_eq!(r, SlotOutcome::Skipped);
    // Icons have no CDN address.
    let r = steam::fetch_and_import(
        &on,
        &http,
        &imp,
        &mut f.db,
        &tracker,
        gid,
        10,
        ArtSlot::Icon,
        true,
        now(),
    )
    .unwrap();
    assert_eq!(r, SlotOutcome::Skipped);
    let _ = FakeClock::baseline();
}

// ---- E2E (network) -----------------------------------------------------------------------------

fn e2e_net() -> bool {
    let on = |k: &str| std::env::var_os(k).is_some_and(|v| v == "1");
    on("ZAUN_E2E") && on("ZAUN_E2E_NET")
}

/// Re-checks assumption V-401 against the real Steam CDN (ADR-0018 §4).
#[test]
fn e2e_steam_cdn_covers() {
    if !e2e_net() {
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let env = PathEnv::new(1000)
        .with("HOME", tmp.path().join("home"))
        .with("XDG_DATA_HOME", tmp.path().join("data"))
        .with("XDG_CACHE_HOME", tmp.path().join("cache"))
        .with("XDG_RUNTIME_DIR", tmp.path().join("run"));
    let paths = AppPaths::resolve(&env).unwrap();
    let fs = zaun_core::fsx::RealFs::new();
    let mut db = Db::open_in_memory().unwrap();
    let http = HttpClient::new(Arc::new(UreqTransport::new()), Arc::new(SystemClock::new()));
    let deps = ImportDeps {
        fs: &fs,
        codec: &C,
        paths: &paths,
        limits: ImageLimits::DEFAULT,
    };
    let tracker = AttemptTracker::new();
    for (n, app_id) in [70u32, 1_145_360, 2_246_340].into_iter().enumerate() {
        match steam::fetch_slot(&steam_on(), &http, &C, app_id, ArtSlot::Cover) {
            FetchOutcome::Fetched { bytes, url } => {
                let info = C.probe(&bytes, &ImageLimits::DEFAULT).unwrap();
                eprintln!(
                    "app {app_id}: {url} → {}×{} ({} bytes)",
                    info.width,
                    info.height,
                    bytes.len()
                );
                assert!(url.ends_with("/library_600x900_2x.jpg"), "V-401: {url}");
            }
            other => panic!("app {app_id}: {other:?}"),
        }
        let g = games::game(10 + n as u32, 1);
        db.insert_game(&g).unwrap();
        let r = steam::fetch_and_import(
            &steam_on(),
            &http,
            &deps,
            &mut db,
            &tracker,
            g.id,
            app_id,
            ArtSlot::Cover,
            false,
            now(),
        )
        .unwrap();
        let SlotOutcome::Imported(i) = r else {
            panic!("app {app_id}: {r:?}")
        };
        assert_eq!(i.art.source, ArtOrigin::SteamCdn);
        assert!(
            i.art.width >= 600 && i.art.height >= 900,
            "app {app_id}: {}×{}",
            i.art.width,
            i.art.height
        );
        eprintln!(
            "app {app_id}: cover {}×{} stored as {}",
            i.art.width, i.art.height, i.art.file
        );
        let h = steam::fetch_and_import(
            &steam_on(),
            &http,
            &deps,
            &mut db,
            &tracker,
            g.id,
            app_id,
            ArtSlot::Hero,
            false,
            now(),
        )
        .unwrap();
        let SlotOutcome::Imported(h) = h else {
            panic!("app {app_id} hero: {h:?}")
        };
        eprintln!("app {app_id}: hero {}×{}", h.art.width, h.art.height);
    }
}

// ---- Property: arbitrary bytes (in-tree companion of fuzz/image_codec) ------------------------

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]
    #[test]
    fn arbitrary_bytes_never_panic(
        prefix in proptest::sample::select(vec![
            b"\x89PNG\r\n\x1a\n".to_vec(), vec![0xFF, 0xD8, 0xFF], b"RIFF\0\0\0\0WEBP".to_vec(),
            b"BM".to_vec(), vec![0, 0, 1, 0, 1, 0], Vec::new(),
        ]),
        tail in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..512),
    ) {
        let mut data = prefix;
        data.extend(tail);
        let limits = ImageLimits { max_bytes: 1 << 20, max_pixels: 1 << 20, max_side: 4096 };
        // Any panic inside a decoder is caught by the codec; one escaping fails the test.
        if let Ok(img) = C.decode(&data, &limits) {
            proptest::prop_assert_eq!(img.pixels.len(), img.width as usize * img.height as usize * 4);
        }
        let _ = thumbs::read_colour_chunk(&data);
        let _ = steam::parse_search(&data);
    }
}
