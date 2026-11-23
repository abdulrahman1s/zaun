//! Friends and requests to become one.

use serde::{Deserialize, Serialize};

use super::{Picture, Playing, Presence};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Friend {
    pub id: i64,
    pub name: String,
    pub ready: bool,
    pub presence: Presence,
    pub playing: Option<Playing>,
    /// Milliseconds since the epoch.
    pub last_seen: Option<i64>,
    pub about: String,
    pub paper: Option<u8>,
    /// Milliseconds since the epoch.
    pub since: Option<i64>,
    pub verified: bool,
    pub blocked: bool,
    pub muted: bool,
    pub seen: Vec<String>,
    pub picture: Option<Picture>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendRequest {
    pub id: i64,
    pub name: String,
}
