//! The worker pool.
//!
//! A job may submit further jobs to its own pool and wait for them ([`JobHandle::wait`]):
//! the host probe fans out its sub-probes, a launch waits for a runtime rescan. With every
//! worker waiting like that nobody would be left to run what they wait for, so a worker that
//! waits for a job still in the queue runs it itself.

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

use super::{CancelToken, JobError, JobLabel, JobPriority};
use crate::clock::Clock;
use crate::consts::{JOB_LABEL_QUEUE_DEFAULT, JOB_WORKERS_MAX, JOB_WORKERS_MIN};
use crate::ids::JobId;

/// `clamp(cores − 1, 2, 6)` (ARCHITECTURE §5).
pub fn default_workers() -> usize {
    let cores = std::thread::available_parallelism().map_or(2, std::num::NonZeroUsize::get);
    cores
        .saturating_sub(1)
        .clamp(JOB_WORKERS_MIN, JOB_WORKERS_MAX)
}

/// Pool configuration.
#[derive(Clone, Debug)]
pub struct JobPoolConfig {
    /// Number of worker threads (at least 1).
    pub workers: usize,
    /// Queue bound per label; labels not listed use `default_bound`.
    pub label_bounds: HashMap<JobLabel, usize>,
    /// Default queue bound per label.
    pub default_bound: usize,
    /// Worker thread name prefix.
    pub thread_name: &'static str,
}

impl Default for JobPoolConfig {
    fn default() -> Self {
        Self {
            workers: default_workers(),
            label_bounds: HashMap::new(),
            default_bound: JOB_LABEL_QUEUE_DEFAULT,
            thread_name: "job",
        }
    }
}

/// Runs the job (`None`) or abandons it with an error (`Some`).
type Task = Box<dyn FnOnce(Option<JobError>) + Send>;

struct Queued {
    label: JobLabel,
    seq: u64,
    task: Task,
}

#[derive(Default)]
struct State {
    queues: [VecDeque<Queued>; 3],
    queued_per_label: HashMap<JobLabel, usize>,
    next_seq: u64,
    shutting_down: bool,
}

struct Shared {
    state: Mutex<State>,
    work: Condvar,
}

thread_local! {
    /// The pool this thread is a worker of (null for every other thread).
    static WORKER_OF: Cell<*const Shared> = const { Cell::new(std::ptr::null()) };
}

/// The result slot shared by a job and its handle.
struct Slot<R> {
    value: Mutex<Option<Result<R, JobError>>>,
    finished: Condvar,
    done: std::sync::atomic::AtomicBool,
}

impl<R> Slot<R> {
    fn new() -> Self {
        Self {
            value: Mutex::new(None),
            finished: Condvar::new(),
            done: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn set(&self, v: Result<R, JobError>) {
        let mut guard = self.value.lock();
        *guard = Some(v);
        self.done.store(true, std::sync::atomic::Ordering::Release);
        self.finished.notify_all();
    }
}

/// A submitted job. Dropping the handle does not cancel the job.
pub struct JobHandle<R> {
    id: JobId,
    label: JobLabel,
    token: CancelToken,
    slot: Arc<Slot<R>>,
    /// Where the job was queued: its pool and queue sequence number (`None`: never queued).
    queued: Option<(Weak<Shared>, u64)>,
}

impl<R> JobHandle<R> {
    /// The job's identifier.
    pub fn id(&self) -> JobId {
        self.id
    }

    /// The job's label.
    pub fn label(&self) -> JobLabel {
        self.label
    }

    /// Request cancellation (the job sees it through its [`CancelToken`]; a queued job does
    /// not start).
    pub fn cancel(&self) {
        self.token.cancel();
    }

    /// The job's cancellation token.
    pub fn cancel_token(&self) -> &CancelToken {
        &self.token
    }

    /// Whether a result is available (or was already taken).
    pub fn is_finished(&self) -> bool {
        self.slot.done.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Take the result if the job finished. Never blocks. Returns `None` before completion
    /// and after the result was taken once.
    pub fn try_take(&self) -> Option<Result<R, JobError>> {
        self.slot.value.lock().take()
    }

    /// Called from a worker of the job's own pool while the job is still queued: run it on
    /// this thread instead of waiting for a free worker (there may never be one).
    fn run_here_if_queued(&self) {
        let Some((pool, seq)) = &self.queued else {
            return;
        };
        let Some(shared) = pool.upgrade() else {
            return;
        };
        if WORKER_OF.get() != Arc::as_ptr(&shared) {
            return;
        }
        let job = take_queued(&mut shared.state.lock(), *seq);
        if let Some(job) = job {
            (job.task)(None);
        }
    }

    /// Block until the job finishes (jobs, the headless CLI and tests; never on the UI
    /// thread). A job waiting for a job of its own pool never starves: see the module text.
    pub fn wait(self) -> Result<R, JobError> {
        self.run_here_if_queued();
        let mut guard = self.slot.value.lock();
        loop {
            if let Some(v) = guard.take() {
                return v;
            }
            if self.slot.done.load(std::sync::atomic::Ordering::Acquire) {
                // Already taken through try_take.
                return Err(JobError::Cancelled);
            }
            self.slot.finished.wait(&mut guard);
        }
    }

    /// Block for at most `timeout`; `None` when the job is still running.
    pub fn wait_timeout(&self, timeout: Duration) -> Option<Result<R, JobError>> {
        let mut guard = self.slot.value.lock();
        if guard.is_none() && !self.slot.done.load(std::sync::atomic::Ordering::Acquire) {
            let _ = self.slot.finished.wait_for(&mut guard, timeout);
        }
        guard.take()
    }
}

/// A fixed-size pool of worker threads with three priority queues.
pub struct JobPool {
    shared: Arc<Shared>,
    clock: Arc<dyn Clock>,
    config: JobPoolConfig,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

impl JobPool {
    /// Start the workers. `clock` provides the time part of job IDs.
    pub fn new(config: JobPoolConfig, clock: Arc<dyn Clock>) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State::default()),
            work: Condvar::new(),
        });
        let mut workers = Vec::new();
        for i in 0..config.workers.max(1) {
            let shared = Arc::clone(&shared);
            let handle = std::thread::Builder::new()
                .name(format!("{}-{i}", config.thread_name))
                .spawn(move || worker_loop(&shared))?;
            workers.push(handle);
        }
        Ok(Self {
            shared,
            clock,
            config,
            workers: Mutex::new(workers),
        })
    }

    /// Number of worker threads.
    pub fn worker_count(&self) -> usize {
        self.workers.lock().len()
    }

    /// Number of queued (not yet started) jobs.
    pub fn queued(&self) -> usize {
        self.shared
            .state
            .lock()
            .queues
            .iter()
            .map(VecDeque::len)
            .sum()
    }

    /// Submit a job. Never blocks.
    pub fn submit<R, F>(&self, priority: JobPriority, label: JobLabel, f: F) -> JobHandle<R>
    where
        R: Send + 'static,
        F: FnOnce(&CancelToken) -> R + Send + 'static,
    {
        self.submit_inner(priority, label, f, None)
    }

    /// Submit a job with a completion callback, run on the worker after the result is stored
    /// (also when the job is abandoned). Typically it posts an internal command to the
    /// AppService.
    pub fn submit_with_callback<R, F, C>(
        &self,
        priority: JobPriority,
        label: JobLabel,
        f: F,
        on_done: C,
    ) -> JobHandle<R>
    where
        R: Send + 'static,
        F: FnOnce(&CancelToken) -> R + Send + 'static,
        C: FnOnce() + Send + 'static,
    {
        self.submit_inner(priority, label, f, Some(Box::new(on_done)))
    }

    fn submit_inner<R, F>(
        &self,
        priority: JobPriority,
        label: JobLabel,
        f: F,
        on_done: Option<Box<dyn FnOnce() + Send>>,
    ) -> JobHandle<R>
    where
        R: Send + 'static,
        F: FnOnce(&CancelToken) -> R + Send + 'static,
    {
        let slot = Arc::new(Slot::new());
        let token = CancelToken::new();
        let mut handle = JobHandle {
            id: JobId::generate(&*self.clock),
            label,
            token: token.clone(),
            slot: Arc::clone(&slot),
            queued: None,
        };
        let task: Task = Box::new(move |abandon: Option<JobError>| {
            let result = match abandon {
                Some(err) => Err(err),
                None if token.is_cancelled() => Err(JobError::Cancelled),
                None => catch_unwind(AssertUnwindSafe(|| f(&token))).map_err(|p| {
                    let message = panic_message(&*p);
                    tracing::error!(
                        code = "EP-INT-001",
                        job = label.as_str(),
                        "job panicked: {message}"
                    );
                    JobError::Panicked { message }
                }),
            };
            slot.set(result);
            if let Some(cb) = on_done {
                let _ = catch_unwind(AssertUnwindSafe(cb));
            }
        });

        let bound = self
            .config
            .label_bounds
            .get(&label)
            .copied()
            .unwrap_or(self.config.default_bound)
            .max(1);
        let mut dropped = None;
        {
            let mut st = self.shared.state.lock();
            if st.shutting_down {
                drop(st);
                task(Some(JobError::ShutDown));
                return handle;
            }
            let count = st.queued_per_label.get(&label).copied().unwrap_or(0);
            if count >= bound {
                dropped = remove_oldest(&mut st, label);
            }
            let seq = st.next_seq;
            st.next_seq += 1;
            handle.queued = Some((Arc::downgrade(&self.shared), seq));
            st.queues[priority.index()].push_back(Queued { label, seq, task });
            *st.queued_per_label.entry(label).or_insert(0) += 1;
        }
        self.shared.work.notify_one();
        if let Some(old) = dropped {
            (old.task)(Some(JobError::Superseded));
        }
        handle
    }

    /// Stop accepting jobs, abandon queued ones with [`JobError::ShutDown`], let running jobs
    /// finish, and join the workers. Idempotent.
    pub fn shutdown(&self) {
        let drained: Vec<Queued> = {
            let mut st = self.shared.state.lock();
            st.shutting_down = true;
            st.queued_per_label.clear();
            st.queues.iter_mut().flat_map(std::mem::take).collect()
        };
        self.shared.work.notify_all();
        for q in drained {
            (q.task)(Some(JobError::ShutDown));
        }
        let workers = std::mem::take(&mut *self.workers.lock());
        let me = std::thread::current().id();
        for w in workers {
            // A job that drops the last owner of its own pool shuts it down from a worker;
            // that worker cannot join itself and exits once its job returns.
            if w.thread().id() != me {
                let _ = w.join();
            }
        }
    }
}

impl Drop for JobPool {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn remove_oldest(st: &mut State, label: JobLabel) -> Option<Queued> {
    let mut best: Option<(usize, usize, u64)> = None;
    for (qi, q) in st.queues.iter().enumerate() {
        if let Some((pos, job)) = q.iter().enumerate().find(|(_, j)| j.label == label)
            && best.is_none_or(|(_, _, seq)| job.seq < seq)
        {
            best = Some((qi, pos, job.seq));
        }
    }
    let (qi, pos, _) = best?;
    let job = st.queues[qi].remove(pos)?;
    if let Some(c) = st.queued_per_label.get_mut(&label) {
        *c = c.saturating_sub(1);
    }
    Some(job)
}

/// Remove the queued job with sequence number `seq`, if it has not started yet.
fn take_queued(st: &mut State, seq: u64) -> Option<Queued> {
    let (qi, pos) = st.queues.iter().enumerate().find_map(|(qi, q)| {
        // Sequence numbers grow along each queue.
        q.binary_search_by_key(&seq, |j| j.seq)
            .ok()
            .map(|pos| (qi, pos))
    })?;
    let job = st.queues[qi].remove(pos)?;
    if let Some(c) = st.queued_per_label.get_mut(&job.label) {
        *c = c.saturating_sub(1);
    }
    Some(job)
}

fn worker_loop(shared: &Shared) {
    WORKER_OF.set(std::ptr::from_ref(shared));
    loop {
        let job = {
            let mut st = shared.state.lock();
            loop {
                if let Some(job) = st.queues.iter_mut().find_map(VecDeque::pop_front) {
                    if let Some(c) = st.queued_per_label.get_mut(&job.label) {
                        *c = c.saturating_sub(1);
                    }
                    break Some(job);
                }
                if st.shutting_down {
                    break None;
                }
                shared.work.wait(&mut st);
            }
        };
        match job {
            Some(job) => (job.task)(None),
            None => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::SystemClock;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc;

    fn pool(workers: usize) -> JobPool {
        JobPool::new(
            JobPoolConfig {
                workers,
                ..JobPoolConfig::default()
            },
            Arc::new(SystemClock::new()),
        )
        .unwrap()
    }

    const L: JobLabel = JobLabel("test");
    const T: Duration = Duration::from_secs(10);

    #[test]
    fn runs_and_returns() {
        let p = pool(2);
        let h = p.submit(JobPriority::Normal, L, |_| 40 + 2);
        assert_eq!(h.wait_timeout(T), Some(Ok(42)));
        assert!(h.is_finished());
        assert_eq!(h.try_take(), None);
    }

    #[test]
    fn default_worker_count_is_clamped() {
        let n = default_workers();
        assert!((2..=6).contains(&n));
    }

    #[test]
    fn panic_is_isolated() {
        let p = pool(1);
        let bad = p.submit(JobPriority::Normal, L, |_| -> u32 { panic!("boom") });
        match bad.wait_timeout(T) {
            Some(Err(JobError::Panicked { message })) => assert_eq!(message, "boom"),
            other => panic!("unexpected {other:?}"),
        }
        // The single worker survived.
        let good = p.submit(JobPriority::Normal, L, |_| 7);
        assert_eq!(good.wait_timeout(T), Some(Ok(7)));
        let err: crate::error::Error = JobError::Panicked {
            message: "x".into(),
        }
        .into();
        assert_eq!(err.code, crate::error::ErrorCode::IntTaskFailed);
    }

    /// Block the only worker until `release` is sent.
    fn block(p: &JobPool) -> (mpsc::Sender<()>, mpsc::Receiver<()>) {
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let _h = p.submit(JobPriority::Interactive, JobLabel("blocker"), move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
        });
        started_rx.recv_timeout(T).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        drop(done_tx);
        (release_tx, done_rx)
    }

    #[test]
    fn higher_priority_runs_first() {
        let p = pool(1);
        let (release, _) = block(&p);
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut handles = Vec::new();
        for (prio, name) in [
            (JobPriority::Background, "bg"),
            (JobPriority::Normal, "n1"),
            (JobPriority::Interactive, "i"),
            (JobPriority::Normal, "n2"),
        ] {
            let order = Arc::clone(&order);
            handles.push(p.submit(prio, L, move |_| order.lock().push(name)));
        }
        release.send(()).unwrap();
        for h in handles {
            h.wait_timeout(T).unwrap().unwrap();
        }
        assert_eq!(*order.lock(), vec!["i", "n1", "n2", "bg"]);
    }

    #[test]
    fn cancellation_is_observable() {
        let p = pool(1);
        let (release, _) = block(&p);
        let queued = p.submit(JobPriority::Normal, L, |_| 1);
        queued.cancel();
        release.send(()).unwrap();
        assert_eq!(queued.wait_timeout(T), Some(Err(JobError::Cancelled)));

        let (tx, rx) = mpsc::channel();
        let running = p.submit(JobPriority::Normal, L, move |tok| {
            tx.send(()).unwrap();
            while !tok.is_cancelled() {
                std::thread::yield_now();
            }
            "stopped"
        });
        rx.recv_timeout(T).unwrap();
        running.cancel();
        assert_eq!(running.wait_timeout(T), Some(Ok("stopped")));
    }

    #[test]
    fn label_bound_replaces_oldest() {
        let mut cfg = JobPoolConfig {
            workers: 1,
            ..JobPoolConfig::default()
        };
        cfg.label_bounds.insert(JobLabel("thumb"), 2);
        let p = JobPool::new(cfg, Arc::new(SystemClock::new())).unwrap();
        let (release, _) = block(&p);
        let a = p.submit(JobPriority::Normal, JobLabel("thumb"), |_| "a");
        let b = p.submit(JobPriority::Normal, JobLabel("thumb"), |_| "b");
        let c = p.submit(JobPriority::Normal, JobLabel("thumb"), |_| "c");
        assert_eq!(a.try_take(), Some(Err(JobError::Superseded)));
        assert_eq!(p.queued(), 2);
        release.send(()).unwrap();
        assert_eq!(b.wait_timeout(T), Some(Ok("b")));
        assert_eq!(c.wait_timeout(T), Some(Ok("c")));
    }

    #[test]
    fn callback_runs_after_result() {
        let p = pool(2);
        let (tx, rx) = mpsc::channel();
        let h = p.submit_with_callback(
            JobPriority::Normal,
            L,
            |_| 5,
            move || {
                tx.send(()).unwrap();
            },
        );
        rx.recv_timeout(T).unwrap();
        assert_eq!(h.try_take(), Some(Ok(5)));
    }

    #[test]
    fn shutdown_abandons_queued_and_finishes_running() {
        let p = pool(1);
        let finished = Arc::new(AtomicUsize::new(0));
        let (release, _) = block(&p);
        let f2 = Arc::clone(&finished);
        let queued = p.submit(JobPriority::Normal, L, move |_| {
            f2.fetch_add(1, Ordering::SeqCst);
        });
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            release.send(()).unwrap();
        });
        p.shutdown();
        releaser.join().unwrap();
        assert_eq!(queued.try_take(), Some(Err(JobError::ShutDown)));
        assert_eq!(finished.load(Ordering::SeqCst), 0);
        let late = p.submit(JobPriority::Normal, L, |_| 1);
        assert_eq!(late.try_take(), Some(Err(JobError::ShutDown)));
        assert_eq!(p.worker_count(), 0);
        p.shutdown();
    }

    #[test]
    fn last_owner_dropped_inside_a_job() {
        let p = Arc::new(pool(2));
        let (go, wait) = mpsc::channel::<()>();
        let owner = Arc::clone(&p);
        let h = p.submit(JobPriority::Normal, L, move |_| {
            wait.recv().unwrap();
            drop(owner);
            7
        });
        drop(p);
        go.send(()).unwrap();
        assert_eq!(h.wait_timeout(T), Some(Ok(7)));
    }

    /// Every worker waits for a job it submitted to its own pool (the host probe and its
    /// sub-probes, a runtime rescan): the queued jobs must still run.
    #[test]
    fn jobs_waiting_for_their_own_jobs_do_not_starve_the_pool() {
        let p = Arc::new(pool(2));
        let gate = Arc::new(std::sync::Barrier::new(2));
        let outer: Vec<_> = (0..2)
            .map(|i| {
                let (inner, gate) = (Arc::clone(&p), Arc::clone(&gate));
                p.submit(JobPriority::Normal, L, move |_| {
                    // Both workers are inside an outer job before either submits.
                    gate.wait();
                    inner.submit(JobPriority::Normal, L, move |_| i * 10).wait()
                })
            })
            .collect();
        for (i, h) in outer.into_iter().enumerate() {
            assert_eq!(h.wait_timeout(T), Some(Ok(Ok(i * 10))));
        }
    }

    #[test]
    fn wait_blocks_until_done() {
        let p = pool(2);
        let h = p.submit(JobPriority::Background, L, |_| {
            std::thread::sleep(Duration::from_millis(20));
            "late"
        });
        assert_eq!(h.wait(), Ok("late"));
    }
}
