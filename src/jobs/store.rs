use crate::error::Result;
use crate::jobs::types::{Job, JobId, JobListFilter, JobStatus, JobSummary};

/// Persistence abstraction for jobs, their arguments, and results/errors.
pub trait JobStore: Send + Sync {
    /// Persist a new pending job; returns its id.
    fn enqueue(&self, job: Job) -> Result<JobId>;

    /// Atomically claim the next pending job for `scan_id` (FIFO), marking it running.
    fn claim_next(&self, scan_id: &str, worker_id: &str) -> Result<Option<Job>>;

    /// Mark a running job completed with a JSON result payload.
    ///
    /// No-op (returns `Ok`) if the job is no longer `Running` (e.g. paused/stopped).
    fn complete(&self, id: &JobId, result: serde_json::Value) -> Result<()>;

    /// Mark a running job failed with an error message (terminal).
    ///
    /// No-op (returns `Ok`) if the job is no longer `Running`.
    fn fail(&self, id: &JobId, error: String) -> Result<()>;

    /// Requeue a running job as `Pending` after a transient failure (retry backoff).
    ///
    /// Stores `error` as the last failure reason, sets `available_at`, and returns
    /// the job to the pending FIFO. No-op if the job is no longer `Running`.
    fn schedule_retry(
        &self,
        id: &JobId,
        error: String,
        available_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()>;

    /// Load a job by id.
    fn get(&self, id: &JobId) -> Result<Option<Job>>;

    /// List jobs matching `filter`, newest `updated_at` first.
    fn list(&self, filter: &JobListFilter) -> Result<Vec<Job>>;

    /// Counts for jobs belonging to `scan_id`.
    fn summary(&self, scan_id: &str) -> Result<JobSummary>;

    /// Counts across all jobs (optionally restricted by status via listing).
    fn summary_all(&self) -> Result<JobSummary> {
        let mut s = JobSummary::default();
        for job in self.list(&JobListFilter::default())? {
            s.record(job.status);
        }
        Ok(s)
    }

    /// Requeue jobs left in `Running` (e.g. after a crash). Returns how many were reclaimed.
    fn reclaim_stale(&self, scan_id: &str) -> Result<u64>;

    /// Reset a job to `Pending` for re-execution.
    ///
    /// Clears result/error/worker timestamps, optionally assigns `new_scan_id`,
    /// and ensures the job is on the pending FIFO.
    fn requeue(&self, id: &JobId, new_scan_id: Option<&str>) -> Result<Job>;

    /// Suspend a pending or running job (`Paused`). Not claimable until unpaused.
    fn pause(&self, id: &JobId) -> Result<Job>;

    /// Resume a paused job to `Pending`.
    fn unpause(&self, id: &JobId) -> Result<Job>;

    /// Cancel a pending, paused, or running job (`Stopped` — terminal).
    fn stop(&self, id: &JobId) -> Result<Job>;

    /// Delete all jobs belonging to `scan_id`. Returns how many were removed.
    fn purge_scan(&self, scan_id: &str) -> Result<u64>;

    /// Delete jobs whose `updated_at` is strictly older than `older_than`.
    /// When `scan_id` is `Some`, only that scan is considered.
    fn purge_older_than(
        &self,
        older_than: chrono::DateTime<chrono::Utc>,
        scan_id: Option<&str>,
    ) -> Result<u64>;

    /// Distinct scan ids present in the store (newest activity first).
    fn list_scan_ids(&self) -> Result<Vec<String>> {
        let mut seen = std::collections::BTreeMap::<String, chrono::DateTime<chrono::Utc>>::new();
        for job in self.list(&JobListFilter::default())? {
            seen.entry(job.scan_id.clone())
                .and_modify(|t| {
                    if job.updated_at > *t {
                        *t = job.updated_at;
                    }
                })
                .or_insert(job.updated_at);
        }
        let mut ids: Vec<_> = seen.into_iter().collect();
        ids.sort_by_key(|b| std::cmp::Reverse(b.1));
        Ok(ids.into_iter().map(|(id, _)| id).collect())
    }

    /// Count jobs in a given status for `scan_id`.
    fn count(&self, scan_id: &str, status: JobStatus) -> Result<u64> {
        let s = self.summary(scan_id)?;
        Ok(match status {
            JobStatus::Pending => s.pending,
            JobStatus::Running => s.running,
            JobStatus::Paused => s.paused,
            JobStatus::Completed => s.completed,
            JobStatus::Failed => s.failed,
            JobStatus::Stopped => s.stopped,
        })
    }
}
