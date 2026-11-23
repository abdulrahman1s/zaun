//! What the native engine carries, known before it starts.

use crate::model::ChatFeatures;

/// The reactions the engine carries, in display form.
pub const REACTION_EMOJI: [&str; 8] = ["👍", "👎", "😀", "😂", "😢", "❤️", "🚀", "✅"];

/// What an engine started with `relays` (`smp://` and `xftp://` addresses; empty = the presets)
/// carries, known before it starts: Settings shows the same rows whether Chat is on or off.
#[must_use]
pub fn features_with(relays: &[String]) -> ChatFeatures {
    let files = has_file_servers(relays);
    ChatFeatures {
        files,
        gifs: files,
        reactions: true,
        replies: true,
        edit: true,
        mute: true,
        block: true,
        verify: true,
        mentions: true,
        calls: true,
        share: true,
        ..ChatFeatures::default()
    }
}

/// The preset file servers go with the preset relays only: whoever picks their own relays has
/// not agreed to the public file servers, so files need a file server of their own too.
#[must_use]
pub fn has_file_servers(relays: &[String]) -> bool {
    let own_relays = relays.iter().any(|r| r.starts_with("smp://"));
    let own_files = relays.iter().any(|r| r.starts_with("xftp://"));
    !own_relays || own_files
}
