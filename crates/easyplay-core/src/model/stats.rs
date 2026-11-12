//! Per-game statistics aggregates (DATA_MODEL §5, STATISTICS).

use serde::{Deserialize, Serialize};

use crate::clock::Timestamp;
use crate::ids::GameId;

/// The `game_stats` row of a game.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameStats {
    /// The game.
    pub game_id: GameId,
    /// Total counted playtime.
    pub total_ms: u64,
    /// Counted sessions.
    pub sessions: u32,
    /// Launches that failed to start.
    pub failed_launches: u32,
    /// First counted session start.
    pub first_played_at: Option<Timestamp>,
    /// Last counted session end (STATISTICS §2).
    pub last_played_at: Option<Timestamp>,
    /// Longest counted session.
    pub longest_ms: u64,
    /// Last launch attempt, counted or not.
    pub last_launch_at: Option<Timestamp>,
}

impl GameStats {
    /// Empty statistics for a game.
    pub fn empty(game_id: GameId) -> Self {
        Self {
            game_id,
            total_ms: 0,
            sessions: 0,
            failed_launches: 0,
            first_played_at: None,
            last_played_at: None,
            longest_ms: 0,
            last_launch_at: None,
        }
    }
}

/// Key of a `playtime_daily` row: a game, or the kept history of a removed game.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameKey {
    /// A game in the library.
    Game(GameId),
    /// A removed game whose history is kept, by name.
    Removed(String),
}

impl GameKey {
    /// Text form for `playtime_daily.game_key`: the game ID, or `removed:<name>`.
    pub fn as_column(&self) -> String {
        match self {
            GameKey::Game(id) => id.to_string(),
            GameKey::Removed(name) => format!("removed:{name}"),
        }
    }

    /// Parse [`GameKey::as_column`].
    pub fn from_column(s: &str) -> Option<Self> {
        match s.strip_prefix("removed:") {
            Some(name) => Some(GameKey::Removed(name.to_owned())),
            None => s.parse().ok().map(GameKey::Game),
        }
    }
}

/// A `playtime_daily` row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaytimeDay {
    /// Game.
    pub game_key: GameKey,
    /// Local calendar date as `YYYYMMDD`.
    pub day: u32,
    /// Playtime on that day.
    pub ms: u64,
    /// Sessions that started on that day.
    pub sessions: u32,
}
