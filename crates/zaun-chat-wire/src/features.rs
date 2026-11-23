//! What the native engine carries, known before it starts.

use crate::model::ChatFeatures;

/// The reactions the engine carries, in display form.
pub const REACTION_EMOJI: [&str; 8] = ["👍", "👎", "😀", "😂", "😢", "❤️", "🚀", "✅"];

/// What an engine started with `relays` carries. The native engine talks to any relay, so
/// the relays do not change it.
#[must_use]
pub fn features_with(relays: &[String]) -> ChatFeatures {
    let _ = relays;
    ChatFeatures {
        files: true,
        gifs: true,
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
