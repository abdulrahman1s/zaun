//! What the engine asks of the network for a file, and how a result comes back (ADR-0012: never on the engine thread).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::Sender;
use zaun_chat_wire::model::ChatRef;

use crate::engine::jobs::JobDone;
use crate::xftp::XftpServer;
use crate::xftp::description::FileDescription;

/// Which file item a job belongs to. `friend` names the conversation: a friend's number, or
/// for a group's file the group's number negated, so the two never meet (both count from 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileKey {
    pub friend: i64,
    pub item: i64,
}

impl FileKey {
    #[must_use]
    pub const fn of(chat: ChatRef, item: i64) -> Self {
        Self {
            friend: owner(chat),
            item,
        }
    }

    #[must_use]
    pub const fn chat(self) -> ChatRef {
        chat_of(self.friend)
    }
}

/// The number a conversation's file records and jobs are keyed by (`FileKey::friend`).
#[must_use]
pub const fn owner(chat: ChatRef) -> i64 {
    match chat {
        ChatRef::Direct(f) => f,
        ChatRef::Group(g) => -g,
    }
}

#[must_use]
pub const fn chat_of(owner: i64) -> ChatRef {
    if owner < 0 {
        ChatRef::Group(-owner)
    } else {
        ChatRef::Direct(owner)
    }
}

pub struct UploadReq {
    pub key: FileKey,
    /// A group's file is made for each of this many members, one description each; `None`
    /// for a friend's.
    pub members: Option<usize>,
    /// Which run this is: a result of an older run of the same file is ignored.
    pub job: u64,
    pub source: PathBuf,
    pub name: String,
    pub work_dir: PathBuf,
    pub servers: Vec<XftpServer>,
    pub cancel: Arc<AtomicBool>,
}

pub struct DownloadReq {
    pub key: FileKey,
    pub job: u64,
    pub description: FileDescription,
    pub work_dir: PathBuf,
    pub dest_dir: PathBuf,
    pub cancel: Arc<AtomicBool>,
}

/// What an upload made for the friend.
#[derive(Debug)]
pub struct Sent {
    /// The recipient description's YAML.
    pub description: String,
    /// When the servers may drop it, epoch seconds.
    pub expires: Option<i64>,
}

/// What an upload made for a group's members: one description each, in the order asked.
#[derive(Debug)]
pub struct SentToMembers {
    pub descriptions: Vec<String>,
    pub expires: Option<i64>,
}

/// A job's result, posted to the engine thread. Progress is already throttled to 5% steps.
#[derive(Debug)]
pub enum FileDone {
    Progress {
        key: FileKey,
        job: u64,
        percent: u8,
    },
    Uploaded {
        key: FileKey,
        job: u64,
        result: Result<Sent, String>,
    },
    /// A group's upload (`UploadReq::members`).
    UploadedForMembers {
        key: FileKey,
        job: u64,
        result: Result<SentToMembers, String>,
    },
    Downloaded {
        key: FileKey,
        job: u64,
        result: Result<PathBuf, String>,
    },
}

/// The network side of files. Each call returns at once; the result arrives on `tx`.
pub trait Transfers: Send {
    fn upload(&mut self, req: UploadReq, tx: Sender<JobDone>);
    fn download(&mut self, req: DownloadReq, tx: Sender<JobDone>);
}

/// Before the start has set files up: every transfer fails at once.
#[derive(Debug, Default)]
pub struct NoTransfers;

impl Transfers for NoTransfers {
    fn upload(&mut self, req: UploadReq, tx: Sender<JobDone>) {
        let result = Err("files are not set up".to_owned());
        let _ = tx.send(JobDone::File(FileDone::Uploaded {
            key: req.key,
            job: req.job,
            result,
        })); // the engine is gone
    }

    fn download(&mut self, req: DownloadReq, tx: Sender<JobDone>) {
        let result = Err("files are not set up".to_owned());
        let _ = tx.send(JobDone::File(FileDone::Downloaded {
            key: req.key,
            job: req.job,
            result,
        })); // the engine is gone
    }
}
