//! A transfer layer that answers at once from what a test scripted.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use crossbeam_channel::Sender;

use crate::engine::files::transfers::{DownloadReq, Sent, UploadReq};
use crate::engine::files::{FileDone, FileKey, Transfers};
use crate::engine::jobs::JobDone;

#[derive(Debug, Default)]
pub struct Script {
    /// Answer each job at once; otherwise it stays running until the test posts a result.
    pub hold: bool,
    pub fail: bool,
    /// The recipient description an upload makes.
    pub description: String,
    /// The name a download saves the file under.
    pub save_as: String,
    pub uploads: Vec<FileKey>,
    pub downloads: Vec<FileKey>,
    pub flags: Vec<(FileKey, Arc<std::sync::atomic::AtomicBool>)>,
    /// The run number of each job started, in order.
    pub jobs: Vec<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct Fake(pub Arc<Mutex<Script>>);

impl Fake {
    pub fn with<R>(&self, f: impl FnOnce(&mut Script) -> R) -> R {
        f(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl Transfers for Fake {
    fn upload(&mut self, req: UploadReq, tx: Sender<JobDone>) {
        let key = req.key;
        let (hold, fail, description) = self.with(|s| {
            s.uploads.push(key);
            s.jobs.push(req.job);
            s.flags.push((key, req.cancel));
            (s.hold, s.fail, s.description.clone())
        });
        if !hold {
            let result = if fail {
                Err("no server".to_owned())
            } else {
                Ok(Sent {
                    description,
                    expires: Some(1_800_000_000),
                })
            };
            tx.send(JobDone::File(FileDone::Uploaded {
                key,
                job: req.job,
                result,
            }))
            .unwrap();
        }
    }

    fn download(&mut self, req: DownloadReq, tx: Sender<JobDone>) {
        let key = req.key;
        let (hold, fail, name) = self.with(|s| {
            s.downloads.push(key);
            s.jobs.push(req.job);
            s.flags.push((key, req.cancel));
            (s.hold, s.fail, s.save_as.clone())
        });
        if !hold {
            let result = if fail {
                Err("no server".to_owned())
            } else {
                let path: PathBuf = req.dest_dir.join(name);
                std::fs::create_dir_all(&req.dest_dir).unwrap();
                std::fs::write(&path, b"fetched").unwrap();
                Ok(path)
            };
            tx.send(JobDone::File(FileDone::Downloaded {
                key,
                job: req.job,
                result,
            }))
            .unwrap();
        }
    }
}
