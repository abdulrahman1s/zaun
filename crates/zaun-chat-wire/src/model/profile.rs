//! The user's profile and how people are around.

use serde::{Deserialize, Serialize};

use super::Picture;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatProfile {
    pub name: String,
    pub about: String,
    pub paper: Option<u8>,
    pub status: MyStatus,
    pub picture: Option<Picture>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MyStatus {
    #[default]
    Online,
    Away,
    Busy,
    Invisible,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Online,
    Away,
    Busy,
    #[default]
    Offline,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playing {
    pub title: String,
    pub minutes: u32,
    pub live: bool,
}
