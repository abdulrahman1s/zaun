//! What a connection tells its owner besides command answers (nook smp/client.ts `ServerEvent`).
use std::sync::Arc;

use super::address::SmpServer;
use super::error::SmpError;
use super::responses::Response;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub version: u16,
    pub session_id: Vec<u8>,
    /// Raw JSON `ServerPublicInfo`, when the server sent it.
    pub server_info: Option<Vec<u8>>,
    pub block_encrypted: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SmpEventKind {
    /// A session is up; the owner re-sends its SUBs here.
    Up(SessionInfo),
    /// The session ended; commands that were waiting have failed.
    Down(SmpError),
    /// A connect attempt failed; the connection retries after a back-off.
    ConnectFailed(SmpError),
    /// MSG, also when it answered SUB, ACK or GET.
    Msg {
        entity_id: Vec<u8>,
        msg_id: Vec<u8>,
        body: Vec<u8>,
    },
    End {
        entity_id: Vec<u8>,
    },
    Deld {
        entity_id: Vec<u8>,
    },
    /// The answer to a command sent with `send_tagged`. A MSG that answers SUB or ACK also arrives as `Msg`.
    Reply {
        tag: u64,
        answer: Result<Response, SmpError>,
    },
    /// A response nobody was waiting for, other than the pushes above.
    Unexpected {
        entity_id: Vec<u8>,
        response: Response,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SmpEvent {
    pub server: Arc<SmpServer>,
    pub kind: SmpEventKind,
}
