//! The XFTP transfers on two worker threads, a cancel flag per job and progress in 5% steps.

use std::fs;
use std::thread;

use crossbeam_channel::{Receiver, Sender, unbounded};

use super::transfers::{DownloadReq, FileDone, FileKey, Sent, SentToMembers, Transfers, UploadReq};
use crate::engine::jobs::JobDone;
use crate::xftp::description::encode_description;
use crate::xftp::{DownloadParams, UploadParams, download, upload};

/// Threads that move files; the rest of the transfers wait in the queue.
const WORKERS: usize = 2;
/// A direct file is made for this many recipients, one of which is used (`roundedFDCount 1`).
const RECIPIENTS: usize = 4;

/// Internal.hs `roundedFDCount`: the next power of two, at least 4.
fn rounded_fd_count(n: usize) -> usize {
    n.max(1).next_power_of_two().max(RECIPIENTS)
}
const STEP: u8 = 5;

type Work = Box<dyn FnOnce() + Send>;

pub struct XftpTransfers {
    queue: Option<Sender<Work>>,
}

impl XftpTransfers {
    #[must_use]
    pub fn new() -> Self {
        Self { queue: None }
    }

    /// Threads start with the first transfer, so an engine that never moves a file has none.
    /// The queue is bounded by the engine, which starts at most `files::MAX_ACTIVE` jobs.
    fn submit(&mut self, work: Work) -> Result<(), String> {
        let tx = self.queue.get_or_insert_with(|| {
            let (tx, rx) = unbounded::<Work>();
            for n in 0..WORKERS {
                spawn(n, rx.clone());
            }
            tx
        });
        tx.send(work).map_err(|_| "no transfer threads".to_owned())
    }
}

impl Default for XftpTransfers {
    fn default() -> Self {
        Self::new()
    }
}

fn spawn(n: usize, rx: Receiver<Work>) {
    let started = thread::Builder::new()
        .name(format!("zaun-xftp-{n}"))
        .spawn(move || rx.iter().for_each(|work| work()));
    // A thread that cannot start leaves the other to do the work, or `submit` to fail.
    drop(started);
}

fn percent(done: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    u8::try_from(done.min(total) * 100 / total).unwrap_or(100)
}

/// Posts progress when it moved a step further than the last one posted.
fn throttled(key: FileKey, job: u64, tx: Sender<JobDone>) -> impl FnMut(u64, u64) {
    let mut last = 0u8;
    move |done, total| {
        let p = percent(done, total);
        if p >= last.saturating_add(STEP) {
            last = p;
            let _ = tx.send(JobDone::File(FileDone::Progress {
                key,
                job,
                percent: p,
            })); // the engine is gone
        }
    }
}

impl Transfers for XftpTransfers {
    fn upload(&mut self, req: UploadReq, tx: Sender<JobDone>) {
        let (key, job) = (req.key, req.job);
        let failed = tx.clone();
        let work: Work = Box::new(move || {
            let done = match req.members {
                None => FileDone::Uploaded {
                    key,
                    job,
                    result: run_upload(&req, &tx),
                },
                Some(n) => FileDone::UploadedForMembers {
                    key,
                    job,
                    result: run_group_upload(&req, n, &tx),
                },
            };
            let _ = tx.send(JobDone::File(done)); // the engine is gone
        });
        if let Err(e) = self.submit(work) {
            let _ = failed.send(JobDone::File(FileDone::Uploaded {
                key,
                job,
                result: Err(e),
            })); // the engine is gone
        }
    }

    fn download(&mut self, req: DownloadReq, tx: Sender<JobDone>) {
        let (key, job) = (req.key, req.job);
        let failed = tx.clone();
        let work: Work = Box::new(move || {
            let result = run_download(&req, &tx);
            let _ = tx.send(JobDone::File(FileDone::Downloaded { key, job, result })); // the engine is gone
        });
        if let Err(e) = self.submit(work) {
            let _ = failed.send(JobDone::File(FileDone::Downloaded {
                key,
                job,
                result: Err(e),
            })); // the engine is gone
        }
    }
}

/// Each recipient description of an upload made for `recipients`, and when it expires.
fn upload_for(
    req: &UploadReq,
    recipients: usize,
    tx: &Sender<JobDone>,
) -> Result<(Vec<String>, Option<i64>), String> {
    fs::create_dir_all(&req.work_dir).map_err(|e| e.to_string())?;
    let params = UploadParams {
        source: &req.source,
        file_name: &req.name,
        work_dir: &req.work_dir,
        servers: &req.servers,
        recipients,
        storage_hours: None,
    };
    let mut progress = throttled(req.key, req.job, tx.clone());
    let up = upload(&params, &req.cancel, &mut progress).map_err(|e| e.to_string())?;
    let all = up.recipients.iter().map(encode_description).collect();
    Ok((all, up.expires))
}

fn run_upload(req: &UploadReq, tx: &Sender<JobDone>) -> Result<Sent, String> {
    let (all, expires) = upload_for(req, RECIPIENTS, tx)?;
    let description = all
        .into_iter()
        .next()
        .ok_or("no description for the friend")?;
    Ok(Sent {
        description,
        expires,
    })
}

/// Subscriber.hs `SFDONE` for a group: a description of its own for each member.
fn run_group_upload(
    req: &UploadReq,
    members: usize,
    tx: &Sender<JobDone>,
) -> Result<SentToMembers, String> {
    let (mut all, expires) = upload_for(req, rounded_fd_count(members), tx)?;
    if all.len() < members {
        return Err("not enough file descriptions for the members".to_owned());
    }
    all.truncate(members);
    Ok(SentToMembers {
        descriptions: all,
        expires,
    })
}

fn run_download(req: &DownloadReq, tx: &Sender<JobDone>) -> Result<std::path::PathBuf, String> {
    fs::create_dir_all(&req.work_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&req.dest_dir).map_err(|e| e.to_string())?;
    let params = DownloadParams {
        description: &req.description,
        work_dir: &req.work_dir,
        dest_dir: &req.dest_dir,
    };
    let mut progress = throttled(req.key, req.job, tx.clone());
    let got = download(&params, &req.cancel, &mut progress).map_err(|e| e.to_string())?;
    Ok(got.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_groups_file_is_made_for_a_power_of_two_of_at_least_four_members() {
        let counts: Vec<usize> = [0, 1, 4, 5, 9].into_iter().map(rounded_fd_count).collect();
        assert_eq!(counts, [4, 4, 4, 8, 16]);
    }

    #[test]
    fn progress_is_posted_in_steps_of_five_percent() {
        let (tx, rx) = unbounded();
        let key = FileKey { friend: 1, item: 2 };
        let mut progress = throttled(key, 1, tx);
        for done in 0..=100 {
            progress(done, 100);
        }
        let seen: Vec<u8> = rx
            .try_iter()
            .filter_map(|j| match j {
                JobDone::File(FileDone::Progress { percent, .. }) => Some(percent),
                JobDone::File(_) | JobDone::Relay(_) => None,
            })
            .collect();
        assert_eq!(seen, (1..=20).map(|n| n * 5).collect::<Vec<u8>>());
    }
}
