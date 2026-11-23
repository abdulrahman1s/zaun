//! What the helper's exit status means (the helper's exit codes: 3 = the database did not
//! open, 4 = a fatal error).

use crate::chat::engine::{ChatEvent, RequestKind};
use crate::error::ErrorCode;

const EXIT_DATABASE: i32 = 3;
const EXIT_FATAL: i32 = 4;

/// The events that end an engine whose helper exited with `status`: a start failure when
/// the status says so (the service stops and says why instead of restarting), then
/// [`ChatEvent::Exited`].
pub(super) fn exit_events(status: Option<i32>, started: bool) -> Vec<ChatEvent> {
    let code = match status {
        Some(EXIT_DATABASE) => Some((ErrorCode::ChtDatabase, "the database did not open")),
        Some(EXIT_FATAL) => Some((ErrorCode::ChtEngineFailing, "the helper hit a fatal error")),
        Some(_) | None => None,
    };
    let mut events = Vec::with_capacity(2);
    if let Some((code, detail)) = code {
        events.push(ChatEvent::Failed {
            request: RequestKind::Start,
            code,
            detail: detail.to_owned(),
        });
    }
    events.push(ChatEvent::Exited { status, started });
    events
}
