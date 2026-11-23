//! What a chat engine can do.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ChatFeatures {
    pub presence: bool,
    pub calls: bool,
    pub group_voice: bool,
    pub share: bool,
    pub voice: bool,
    pub gifs: bool,
    pub files: bool,
    pub reactions: bool,
    pub replies: bool,
    pub edit: bool,
    pub mute: bool,
    pub block: bool,
    pub verify: bool,
    pub mentions: bool,
    pub link_previews: bool,
}
