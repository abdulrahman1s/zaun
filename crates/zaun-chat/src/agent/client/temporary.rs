//! Which SMP failures are worth another attempt (Agent/Client.hs `temporaryOrHostError`).
use crate::smp::SmpError;

/// The network or the session failed, so the same command may succeed later; a refusal or a bad command will not.
#[must_use]
pub(crate) fn is_temporary(e: &SmpError) -> bool {
    match e {
        SmpError::Io(_)
        | SmpError::Timeout
        | SmpError::NotConnected
        | SmpError::Unresponsive
        | SmpError::Closed
        | SmpError::Busy
        | SmpError::TooManyPending
        | SmpError::Handshake(_)
        | SmpError::Decode(_) => true,
        SmpError::Server(_)
        | SmpError::Encode(_)
        | SmpError::Crypto(_)
        | SmpError::Unexpected(_)
        | SmpError::LargeMsg
        | SmpError::Address(_) => false,
    }
}
