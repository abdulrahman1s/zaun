//! The overlay's pipe protocol, version 2 (ADR-0037 §3): one JSON object per line, UTF-8,
//! on the overlay's standard input and output only. Every line carries `"v"`; a line of
//! another version or shape is dropped (logged), never the pipe.
//!
//! ```text
//! app → overlay   {"v":2,"t":"state","state":{…OverlaySnapshot…}}   {"v":2,"t":"open"}   {"v":2,"t":"close"}
//! overlay → app   {"v":2,"t":"ready"}   {"v":2,"t":"send_message","chat":{"direct":3},"text":"gg"}
//!                 {"v":2,"t":"open_conversation","chat":{"group":7}}   {"v":2,"t":"call_mute","on":true}
//!                 {"v":2,"t":"call_deafen","on":false}   {"v":2,"t":"end_call"}   {"v":2,"t":"closed"}
//! ```
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::snapshot::{ChatKey, Corner, OverlaySnapshot};

/// The protocol version both ends speak (2: the ported design's snapshot).
pub const VERSION: u32 = 2;

/// Exit status: this desktop has no layer shell (GNOME). Starting again would not help.
pub const EXIT_UNSUPPORTED: i32 = 3;
/// Exit status: no GPU path that can draw a transparent surface. Not retried either.
pub const EXIT_NO_GPU: i32 = 4;
/// Exit status: started without `WAYLAND_DISPLAY` (it never guesses a display).
pub const EXIT_NO_DISPLAY: i32 = 2;

/// What the app tells the overlay.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ToOverlay {
    /// Everything the overlay draws, replacing the last one.
    State { state: Box<OverlaySnapshot> },
    /// Show the panel and take the keyboard (the hotkey).
    Open,
    /// Hide the panel and give input back to the game.
    Close,
}

/// What the overlay tells the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum FromOverlay {
    /// Connected to the compositor with its surface; send it state.
    Ready,
    SendMessage {
        chat: ChatKey,
        text: String,
    },
    /// The user picked a conversation: its latest messages are wanted.
    OpenConversation {
        chat: ChatKey,
    },
    CallMute {
        on: bool,
    },
    CallDeafen {
        on: bool,
    },
    EndCall,
    /// Answer or decline the call that rings (the answer bar).
    Answer,
    Decline,
    /// Share the game in the call's conversation, or stop.
    Share {
        on: bool,
    },
    /// The "Show while playing" popover changed the overlay's settings (the same keys as
    /// Settings › Overlay: one setting, two places).
    SetLook {
        toasts: bool,
        voice_widget: bool,
        corner: Corner,
    },
    /// The user closed the panel from inside (Escape).
    Closed,
}

/// Why a line was dropped.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LineError {
    #[error("protocol version {0}, expected {VERSION}")]
    Version(u32),
    #[error("malformed line: {0}")]
    Malformed(String),
}

#[derive(Serialize)]
struct Out<'a, T> {
    v: u32,
    #[serde(flatten)]
    msg: &'a T,
}

#[derive(Deserialize)]
struct Envelope {
    v: u32,
}

/// One line (without the newline).
pub fn encode<T: Serialize>(msg: &T) -> String {
    // Serialising these types cannot fail: no maps with non-string keys, no foreign types.
    serde_json::to_string(&Out { v: VERSION, msg }).unwrap_or_default()
}

/// Read one line: its version first, then the message.
pub fn decode<T: DeserializeOwned>(line: &str) -> Result<T, LineError> {
    let env: Envelope =
        serde_json::from_str(line).map_err(|e| LineError::Malformed(e.to_string()))?;
    if env.v != VERSION {
        return Err(LineError::Version(env.v));
    }
    serde_json::from_str(line).map_err(|e| LineError::Malformed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::overlay::snapshot::OverlayLook;

    #[test]
    fn every_message_survives_a_round_trip_through_one_line() {
        let to = [
            ToOverlay::Open,
            ToOverlay::Close,
            ToOverlay::State {
                state: Box::new(OverlaySnapshot {
                    look: OverlayLook {
                        toasts: true,
                        corner: Corner::BottomRight,
                        key: "Shift+F1".into(),
                        ..OverlayLook::default()
                    },
                    ..OverlaySnapshot::default()
                }),
            },
        ];
        for m in to {
            let line = encode(&m);
            assert!(!line.contains('\n'));
            assert_eq!(decode::<ToOverlay>(&line), Ok(m));
        }
        let from = [
            FromOverlay::Ready,
            FromOverlay::SendMessage {
                chat: ChatKey::Direct(3),
                text: "gg\nwp".into(),
            },
            FromOverlay::OpenConversation {
                chat: ChatKey::Group(7),
            },
            FromOverlay::CallMute { on: true },
            FromOverlay::CallDeafen { on: false },
            FromOverlay::EndCall,
            FromOverlay::Answer,
            FromOverlay::Decline,
            FromOverlay::Share { on: true },
            FromOverlay::SetLook {
                toasts: false,
                voice_widget: true,
                corner: Corner::TopRight,
            },
            FromOverlay::Closed,
        ];
        for m in from {
            let line = encode(&m);
            assert!(!line.contains('\n'));
            assert_eq!(decode::<FromOverlay>(&line), Ok(m));
        }
    }

    #[test]
    fn the_wire_names_are_the_documented_ones() {
        assert_eq!(encode(&ToOverlay::Open), r#"{"v":2,"t":"open"}"#);
        assert_eq!(
            encode(&FromOverlay::SendMessage {
                chat: ChatKey::Direct(3),
                text: "gg".into()
            }),
            r#"{"v":2,"t":"send_message","chat":{"direct":3},"text":"gg"}"#
        );
        assert_eq!(
            encode(&FromOverlay::CallMute { on: true }),
            r#"{"v":2,"t":"call_mute","on":true}"#
        );
    }

    #[test]
    fn a_line_of_another_version_is_refused_before_it_is_read() {
        assert_eq!(
            decode::<FromOverlay>(r#"{"v":1,"t":"ready"}"#),
            Err(LineError::Version(1))
        );
        // Even when the rest would not parse as version 1.
        assert_eq!(
            decode::<FromOverlay>(r#"{"v":9,"t":"teleport"}"#),
            Err(LineError::Version(9))
        );
    }

    #[test]
    fn a_malformed_line_is_an_error_not_a_panic() {
        for line in [
            "",
            "not json",
            r#"{"t":"ready"}"#,
            r#"{"v":2}"#,
            r#"{"v":2,"t":"send_message","chat":{"direct":"x"},"text":"a"}"#,
            r#"{"v":2,"t":"unknown"}"#,
        ] {
            assert!(
                matches!(decode::<FromOverlay>(line), Err(LineError::Malformed(_))),
                "{line}"
            );
        }
    }
}
