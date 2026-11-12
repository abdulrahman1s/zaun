//! Sample library and a preview [`Backend`] used when no application service is connected
//! (`easyplay` before the service lands) and by snapshot tests. Titles are invented.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use easyplay_core::clock::Timestamp;
use easyplay_core::ids::{GameId, TagId, TagName};
use easyplay_core::model::{
    Activity, ArtKey, Availability, BasePreset, EntryKind, GameSummary, LaunchStateTag,
    NetworkAccess, Preset, SaveLocationKind, SummaryFlags, TagColor, sort_name_of,
};

use crate::bridge::{Backend, HostStatus, LibraryView, TagInfo, UiCommand, UiEvent};
use crate::strings;

/// 2026-11-12T19:30:00Z: the specification baseline, evening.
pub fn baseline_now() -> Timestamp {
    Timestamp::from_millis(1_794_441_600_000 + 19 * 3_600_000 + 30 * 60_000)
}

const H: u64 = 3_600_000;
const M: u64 = 60_000;
const DAY: u64 = 24 * H;

struct Spec {
    name: &'static str,
    preset: Preset,
    played_ms: u64,
    last_days: Option<u64>,
    favorite: bool,
    art: bool,
    tags: &'static [i64],
    activity: Activity,
    availability: Availability,
    kind: EntryKind,
    attention: bool,
}

const fn spec(name: &'static str, preset: Preset, played_ms: u64, last_days: Option<u64>) -> Spec {
    Spec {
        name,
        preset,
        played_ms,
        last_days,
        favorite: false,
        art: true,
        tags: &[],
        activity: Activity::Idle,
        availability: Availability::Present,
        kind: EntryKind::Game,
        attention: false,
    }
}

fn specs(now: Timestamp) -> Vec<Spec> {
    let running_since = now.saturating_sub(Duration::from_millis(42 * M + 10_000));
    vec![
        Spec {
            favorite: true,
            tags: &[1],
            ..spec("Lantern Keep", Preset::Offline, 12 * H + 5 * M, Some(1))
        },
        Spec {
            activity: Activity::Running {
                since: running_since,
            },
            tags: &[2],
            ..spec("Starfall Tactics", Preset::Online, 31 * H + 40 * M, Some(0))
        },
        Spec {
            favorite: true,
            ..spec("Moss & Iron", Preset::Offline, 4 * H + 12 * M, Some(3))
        },
        Spec {
            activity: Activity::Launching(LaunchStateTag::Preparing),
            ..spec("Harbor Lights", Preset::Standard, 55 * M, Some(9))
        },
        spec(
            "Quiet Orbit",
            Preset::Custom {
                base: BasePreset::Offline,
            },
            128 * H,
            Some(16),
        ),
        Spec {
            art: false,
            ..spec("Vale of Cinders", Preset::Offline, 0, None)
        },
        Spec {
            availability: Availability::Missing,
            ..spec("Paper Kingdoms", Preset::Online, 7 * H, Some(40))
        },
        Spec {
            tags: &[1],
            ..spec(
                "The Long Winter Road: Director's Edition",
                Preset::Offline,
                22 * H + 30 * M,
                Some(5),
            )
        },
        Spec {
            availability: Availability::DriveDisconnected,
            ..spec("Ember Run", Preset::Standard, 3 * H, Some(70))
        },
        Spec {
            kind: EntryKind::Installer,
            art: false,
            ..spec("Setup Glassworks", Preset::Offline, 0, None)
        },
        Spec {
            attention: true,
            ..spec("Tidewater", Preset::Online, 18 * M, Some(2))
        },
        Spec {
            favorite: true,
            tags: &[2, 3],
            ..spec("Ninefold Arena", Preset::Online, 410 * H, Some(4))
        },
        spec("Clockwork Garden", Preset::Offline, 2 * H + 2 * M, Some(12)),
        Spec {
            art: false,
            ..spec("Driftwood", Preset::Offline, 40 * M, Some(20))
        },
        Spec {
            tags: &[3],
            ..spec("Signal Lost", Preset::Online, 9 * H, Some(28))
        },
        spec("Amber Sky", Preset::Standard, 1240 * H, Some(1)),
        spec("Northbound", Preset::Offline, 6 * H, Some(100)),
        Spec {
            tags: &[1],
            ..spec("Rune & Rail", Preset::Offline, 0, None)
        },
        spec(
            "Hollow Bell",
            Preset::Custom {
                base: BasePreset::Online,
            },
            15 * H,
            Some(6),
        ),
        spec("Kestrel", Preset::Offline, H, Some(33)),
        spec("Low Tide Motel", Preset::Offline, 3 * H + 30 * M, Some(2)),
        spec("Graywater", Preset::Online, 0, None),
        spec("Saltmarsh", Preset::Offline, 26 * H, Some(11)),
        spec("Wildflower Pass", Preset::Offline, 8 * H, Some(18)),
    ]
}

fn art_key(seed: u64) -> ArtKey {
    let mut h = [0u8; 32];
    for (i, b) in h.iter_mut().enumerate() {
        *b = (seed.rotate_left(i as u32 * 7) ^ (i as u64 * 0x9E37)) as u8;
    }
    h[0] = seed as u8;
    h[1] = (seed >> 8) as u8;
    ArtKey(h)
}

/// The sample library at `now`.
pub fn library(now: Timestamp) -> LibraryView {
    let tag = |id: i64, name: &str, color: u8| TagInfo {
        id: TagId::new(id),
        name: TagName::new(name).map_or_else(|_| Arc::from(name), |n| Arc::from(n.as_str())),
        color: TagColor::new(color).unwrap_or_default(),
    };
    let tags: Arc<[TagInfo]> = vec![
        tag(2, "co-op", 0),
        tag(3, "competitive", 6),
        tag(1, "rpg", 1),
    ]
    .into();
    let mut games: Vec<GameSummary> = specs(now)
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let last = s
                .last_days
                .map(|d| now.saturating_sub(Duration::from_millis(d * DAY + (i as u64 % 5) * H)));
            let sessions = if s.played_ms > 0 {
                (s.played_ms / H) as u32 + 1
            } else {
                0
            };
            GameSummary {
                id: GameId::from_uuid(uuid::Uuid::from_u128(0x1000 + i as u128)),
                name: Arc::from(s.name),
                sort_name: Arc::from(sort_name_of(s.name)),
                cover: s.art.then(|| art_key(i as u64 + 1)),
                icon: None,
                favorite: s.favorite,
                hidden: false,
                tags: s.tags.iter().map(|t| TagId::new(*t)).collect(),
                added_at: now.saturating_sub(Duration::from_millis((i as u64 + 3) * 9 * DAY)),
                last_played_at: last,
                first_played_at: last,
                total_playtime_ms: s.played_ms,
                session_count: sessions,
                launch_count: sessions,
                preset: s.preset,
                network: if matches!(
                    s.preset,
                    Preset::Offline
                        | Preset::Custom {
                            base: BasePreset::Offline
                        }
                ) {
                    NetworkAccess::None
                } else {
                    NetworkAccess::Full
                },
                runtime_label: Arc::from("UMU-Proton 10.0-4"),
                primary_exe_path: Arc::from(Path::new("/games/sample/game.exe")),
                root: None,
                save_location: SaveLocationKind::Internal,
                kind: s.kind,
                availability: s.availability,
                activity: s.activity,
                flags: SummaryFlags {
                    needs_attention: s.attention,
                    ..SummaryFlags::default()
                },
            }
        })
        .collect();
    games.sort_by(|a, b| a.sort_name.cmp(&b.sort_name));
    LibraryView {
        revision: 1,
        games: games.into(),
        tags,
    }
}

struct DemoState {
    library: Arc<LibraryView>,
    events: std::collections::VecDeque<UiEvent>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// A preview backend over the sample library. Favourites toggle for real; launching and
/// stopping answer with a toast, since nothing can run without the service.
pub struct DemoBackend {
    state: Mutex<DemoState>,
}

impl DemoBackend {
    /// Sample data as of `now`.
    pub fn new(now: Timestamp) -> Self {
        Self {
            state: Mutex::new(DemoState {
                library: Arc::new(library(now)),
                events: Default::default(),
                waker: None,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DemoState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Backend for DemoBackend {
    fn send(&self, cmd: UiCommand) {
        let mut s = self.lock();
        let name_of = |s: &DemoState, id| {
            s.library
                .game(id)
                .map(|g| g.name.to_string())
                .unwrap_or_default()
        };
        match cmd {
            UiCommand::SetFavorite(id, on) => {
                let mut lib = (*s.library).clone();
                let mut games = lib.games.to_vec();
                if let Some(g) = games.iter_mut().find(|g| g.id == id) {
                    g.favorite = on;
                }
                lib.games = games.into();
                lib.revision += 1;
                s.library = Arc::new(lib);
                s.events.push_back(UiEvent::LibraryChanged);
            }
            UiCommand::Launch(id) => {
                let msg = strings::feedback::preview_action(strings::play::PLAY, &name_of(&s, id));
                s.events.push_back(UiEvent::Toast(msg));
            }
            UiCommand::Stop(id) => {
                let msg = strings::feedback::preview_action(strings::play::STOP, &name_of(&s, id));
                s.events.push_back(UiEvent::Toast(msg));
            }
            UiCommand::OpenPaths(_) => {}
        }
        if let Some(w) = &s.waker {
            w();
        }
    }

    fn try_event(&self) -> Option<UiEvent> {
        self.lock().events.pop_front()
    }

    fn library(&self) -> Arc<LibraryView> {
        self.lock().library.clone()
    }

    fn host_status(&self) -> HostStatus {
        HostStatus::Ready
    }

    fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.lock().waker = Some(waker);
    }

    fn is_preview(&self) -> bool {
        true
    }
}
