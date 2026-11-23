//! What the engine asks of the network for a file, and how a result comes back (ADR-0012: never on the engine thread).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossbeam_channel::Sender;

use crate::engine::jobs::JobDone;
use crate::xftp::XftpServer;
use crate::xftp::description::FileDescription;

/// Which file item a job belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileKey {
    pub friend: i64,
    pub item: i64,
}

pub struct UploadReq {
    pub key: FileKey,
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
