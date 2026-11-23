//! The user's profile record (Types.hs `Profile` as Zaun keeps it; the paper and status stay local).

use serde::{Deserialize, Serialize};
use zaun_chat_wire::model::{ChatProfile, MyStatus, Picture};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRecord {
    pub name: String,
    pub about: String,
    pub paper: Option<u8>,
    pub status: MyStatus,
    pub picture: Option<Picture>,
}

impl UserRecord {
    #[must_use]
    pub fn named(name: &str) -> Self {
        Self {
            name: name.trim().to_owned(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn to_wire(&self) -> ChatProfile {
        ChatProfile {
            name: self.name.clone(),
            about: self.about.clone(),
            paper: self.paper,
            status: self.status,
            picture: self.picture.clone(),
        }
    }
}
