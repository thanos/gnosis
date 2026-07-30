use crate::error::{GnosisError, Result};
use crate::jobs::retry::RetryPolicy;
use crate::jobs::store::JobStore;
use crate::jobs::types::Job;
use chrono::{Duration as ChronoDuration, Utc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Executes a claimed job and returns a JSON result on success.
pub trait JobExecutor: Send + Sync {
    fn execute(&self, job: &Job) -> Result<serde_json::Value>;
}

/// Async worker pool that claims jobs from a [`JobStore`] and runs a [`JobExecutor`].
pub struct JobWorkerPool {
    store: Arc<dyn JobStore>,
    executor: Arc<dyn JobExecutor>,
    scan_id: String,
    cancel: Arc<AtomicBool>,
    /// When true, workers exit once the queue is drained (no pending/running).
    enqueue_done: Arc<AtomicBool>,
    poll_interval: Duration,
    concurrency: usize,
    retry: RetryPolicy,
}

impl JobWorkerPool {
    pub fn new(
        store: Arc<dyn JobStore>,
        executor: Arc<dyn JobExecutor>,
        scan_id: impl Into<String>,
        cancel: Arc<AtomicBool>,
        concurrency: usize,
        poll_interval: Duration,
    ) -> Self {
        Self::with_retry(
            store,
            executor,
            scan_id,
            cancel,
            concurrency,
            poll_interval,
            RetryPolicy::default(),
        )
    }

    pub fn with_retry(
        store: Arc<dyn JobStore>,
        executor: Arc<dyn JobExecutor>,
        scan_id: impl Into<String>,
        cancel: Arc<AtomicBool>,
        concurrency: usize,
        poll_interval: Duration,
        retry: RetryPolicy,
    ) -> Self {
        Self {
            store,
            executor,
            scan_id: scan_id.into(),
            cancel,
            enqueue_done: Arc::new(AtomicBool::new(false)),
            poll_interval,
            concurrency: concurrency.max(1),
            retry,
        }
    }

    pub fn enqueue_done_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.enqueue_done)
    }

    pub fn mark_enqueue_done(&self) {
        self.enqueue_done.store(true, Ordering::Relaxed);
    }

    /// Run workers on the current tokio runtime until the queue drains or cancel is set.
    pub async fn run(self) -> Result<()> {
        let mut handles = Vec::new();
        for i in 0..self.concurrency {
            let store = Arc::clone(&self.store);
            let executor = Arc::clone(&self.executor);
            let scan_id = self.scan_id.clone();
            let cancel = Arc::clone(&self.cancel);
            let enqueue_done = Arc::clone(&self.enqueue_done);
            let poll = self.poll_interval;
            let retry = self.retry;
            let worker_id = format!("worker-{i}");
            handles.push(tokio::spawn(async move {
                worker_loop(
                    store,
                    executor,
                    scan_id,
                    worker_id,
                    cancel,
                    enqueue_done,
                    poll,
                    retry,
                )
                .await
            }));
        }

        for h in handles {
            h.await
                .map_err(|e| GnosisError::Job(format!("worker join: {e}")))??;
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
async fn worker_loop(
    store: Arc<dyn JobStore>,
    executor: Arc<dyn JobExecutor>,
    scan_id: String,
    worker_id: String,
    cancel: Arc<AtomicBool>,
    enqueue_done: Arc<AtomicBool>,
    poll: Duration,
    retry: RetryPolicy,
) -> Result<()> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }

        let claimed = {
            let store = Arc::clone(&store);
            let scan_id = scan_id.clone();
            let worker_id = worker_id.clone();
            tokio::task::spawn_blocking(move || store.claim_next(&scan_id, &worker_id))
                .await
                .map_err(|e| GnosisError::Job(format!("claim join: {e}")))??
        };

        match claimed {
            Some(job) => {
                let job_id = job.id.clone();
                let attempts = job.attempts;
                let exec = Arc::clone(&executor);
                let outcome = tokio::task::spawn_blocking(move || exec.execute(&job))
                    .await
                    .map_err(|e| GnosisError::Job(format!("execute join: {e}")))?;

                let store = Arc::clone(&store);
                match outcome {
                    Ok(result) => {
                        tokio::task::spawn_blocking(move || store.complete(&job_id, result))
                            .await
                            .map_err(|e| GnosisError::Job(format!("complete join: {e}")))??;
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if retry.should_retry(attempts) {
                            let delay = retry.delay_after(attempts);
                            let available_at = Utc::now()
                                + ChronoDuration::from_std(delay).unwrap_or_else(|_| {
                                    ChronoDuration::milliseconds(delay.as_millis() as i64)
                                });
                            let msg = format!(
                                "attempt {attempts}/{} failed: {msg}; retrying after {}ms",
                                retry.max_attempts,
                                delay.as_millis()
                            );
                            tokio::task::spawn_blocking(move || {
                                store.schedule_retry(&job_id, msg, available_at)
                            })
                            .await
                            .map_err(|e| GnosisError::Job(format!("retry join: {e}")))??;
                        } else {
                            let msg =
                                format!("attempt {attempts}/{} failed: {msg}", retry.max_attempts);
                            tokio::task::spawn_blocking(move || store.fail(&job_id, msg))
                                .await
                                .map_err(|e| GnosisError::Job(format!("fail join: {e}")))??;
                        }
                    }
                }
            }
            None => {
                if enqueue_done.load(Ordering::Relaxed) {
                    let store = Arc::clone(&store);
                    let scan_id = scan_id.clone();
                    let summary = tokio::task::spawn_blocking(move || store.summary(&scan_id))
                        .await
                        .map_err(|e| GnosisError::Job(format!("summary join: {e}")))??;
                    if summary.active() == 0 {
                        break;
                    }
                }
                tokio::time::sleep(poll).await;
            }
        }
    }
    Ok(())
}
