//! The media threads' time. str0m and the sound pacing need `std::time::Instant`; the
//! service's `Clock` is for the service thread and its tests.
use std::time::Instant;

#[allow(
    clippy::disallowed_methods,
    reason = "str0m is driven by std Instants; the media threads keep wall time"
)]
pub(crate) fn now() -> Instant {
    Instant::now()
}
