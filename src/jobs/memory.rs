use crate::error::{GnosisError, Result};
use crate::jobs::store::JobStore;
use crate::jobs::types::{Job, JobId, JobStatus, JobSummary};
use chrono::Utc;
use std::collections::VecDeque;
use std::sync::Mutex;

/// In-memory [`JobStore`] for tests and ephemeral runs.
#[derive(Default)]
pub struct MemoryJobStore {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    jobs: Vec<Job>,
    /// FIFO of pending job ids (as strings).
    pending: VecDeque<String>,
}

impl MemoryJobStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn remove_from_pending(inner: &mut Inner, id: &str) {
        inner.pending.retain(|p| p != id);
    }

    fn ensure_pending(inner: &mut Inner, id: &str) {
        if !inner.pending.iter().any(|p| p == id) {
            inner.pending.push_back(id.to_string());
        }
    }
}

impl JobStore for MemoryJobStore {
    fn enqueue(&self, job: Job) -> Result<JobId> {
        let mut inner = self.inner.lock().unwrap();
        let id = job.id.clone();
        inner.pending.push_back(id.as_str().to_string());
        inner.jobs.push(job);
        Ok(id)
    }

    fn claim_next(&self, scan_id: &str, worker_id: &str) -> Result<Option<Job>> {
        let mut inner = self.inner.lock().unwrap();
        let now = Utc::now();
        let mut idx_in_pending = None;
        for (i, pid) in inner.pending.iter().enumerate() {
            if let Some(job) = inner.jobs.iter().find(|j| j.id.as_str() == pid) {
                if job.scan_id == scan_id && job.is_claimable_at(now) {
                    idx_in_pending = Some(i);
                    break;
                }
            }
        }
        let Some(pi) = idx_in_pending else {
            return Ok(None);
        };
        let job_id = inner.pending.remove(pi).unwrap();
        let job = inner
            .jobs
            .iter_mut()
            .find(|j| j.id.as_str() == job_id)
            .ok_or_else(|| GnosisError::Job(format!("missing job {job_id}")))?;
        job.status = JobStatus::Running;
        job.attempts += 1;
        job.started_at = Some(now);
        job.updated_at = now;
        job.available_at = None;
        job.worker_id = Some(worker_id.to_string());
        Ok(Some(job.clone()))
    }

    fn complete(&self, id: &JobId, result: serde_json::Value) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let job = inner
            .jobs
            .iter_mut()
            .find(|j| &j.id == id)
            .ok_or_else(|| GnosisError::Job(format!("missing job {id}")))?;
        if job.status != JobStatus::Running {
            return Ok(());
        }
        let now = Utc::now();
        job.status = JobStatus::Completed;
        job.result = Some(result);
        job.error = None;
        job.finished_at = Some(now);
        job.updated_at = now;
        Ok(())
    }

    fn fail(&self, id: &JobId, error: String) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let job = inner
            .jobs
            .iter_mut()
            .find(|j| &j.id == id)
            .ok_or_else(|| GnosisError::Job(format!("missing job {id}")))?;
        if job.status != JobStatus::Running {
            return Ok(());
        }
        let now = Utc::now();
        job.status = JobStatus::Failed;
        job.error = Some(error);
        job.finished_at = Some(now);
        job.updated_at = now;
        job.available_at = None;
        Ok(())
    }

    fn schedule_retry(
        &self,
        id: &JobId,
        error: String,
        available_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        let now = Utc::now();
        let id_str = {
            let job = inner
                .jobs
                .iter_mut()
                .find(|j| &j.id == id)
                .ok_or_else(|| GnosisError::Job(format!("missing job {id}")))?;
            if job.status != JobStatus::Running {
                return Ok(());
            }
            job.status = JobStatus::Pending;
            job.error = Some(error);
            job.result = None;
            job.worker_id = None;
            job.finished_at = None;
            job.available_at = Some(available_at);
            job.updated_at = now;
            job.id.as_str().to_string()
        };
        Self::ensure_pending(&mut inner, &id_str);
        Ok(())
    }

    fn get(&self, id: &JobId) -> Result<Option<Job>> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.jobs.iter().find(|j| &j.id == id).cloned())
    }

    fn list(&self, filter: &crate::jobs::types::JobListFilter) -> Result<Vec<Job>> {
        let inner = self.inner.lock().unwrap();
        let mut jobs: Vec<Job> = inner
            .jobs
            .iter()
            .filter(|j| {
                if let Some(scan) = &filter.scan_id {
                    if j.scan_id != *scan {
                        return false;
                    }
                }
                if let Some(status) = filter.status {
                    if j.status != status {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();
        jobs.sort_by_key(|b| std::cmp::Reverse(b.updated_at));
        if let Some(limit) = filter.limit {
            jobs.truncate(limit);
        }
        Ok(jobs)
    }

    fn summary(&self, scan_id: &str) -> Result<JobSummary> {
        let inner = self.inner.lock().unwrap();
        let mut s = JobSummary::default();
        for job in inner.jobs.iter().filter(|j| j.scan_id == scan_id) {
            s.record(job.status);
        }
        Ok(s)
    }

    fn reclaim_stale(&self, scan_id: &str) -> Result<u64> {
        let mut inner = self.inner.lock().unwrap();
        let mut n = 0u64;
        let now = Utc::now();
        let mut requeue = Vec::new();
        for job in inner.jobs.iter_mut() {
            if job.scan_id == scan_id && job.status == JobStatus::Running {
                job.status = JobStatus::Pending;
                job.worker_id = None;
                job.started_at = None;
                job.updated_at = now;
                requeue.push(job.id.as_str().to_string());
                n += 1;
            }
        }
        for id in requeue {
            Self::ensure_pending(&mut inner, &id);
        }
        Ok(n)
    }

    fn requeue(&self, id: &JobId, new_scan_id: Option<&str>) -> Result<Job> {
        let mut inner = self.inner.lock().unwrap();
        let now = Utc::now();
        let (id_str, job) = {
            let job = inner
                .jobs
                .iter_mut()
                .find(|j| &j.id == id)
                .ok_or_else(|| GnosisError::Job(format!("missing job {id}")))?;
            if let Some(scan) = new_scan_id {
                job.scan_id = scan.to_string();
            }
            job.status = JobStatus::Pending;
            job.result = None;
            job.error = None;
            job.worker_id = None;
            job.started_at = None;
            job.finished_at = None;
            job.available_at = None;
            job.updated_at = now;
            (job.id.as_str().to_string(), job.clone())
        };
        Self::ensure_pending(&mut inner, &id_str);
        Ok(job)
    }

    fn pause(&self, id: &JobId) -> Result<Job> {
        let mut inner = self.inner.lock().unwrap();
        let now = Utc::now();
        let id_str = id.as_str().to_string();
        let job = {
            let job = inner
                .jobs
                .iter_mut()
                .find(|j| &j.id == id)
                .ok_or_else(|| GnosisError::Job(format!("missing job {id}")))?;
            match job.status {
                JobStatus::Pending | JobStatus::Running => {
                    job.status = JobStatus::Paused;
                    job.worker_id = None;
                    job.available_at = None;
                    job.updated_at = now;
                    job.clone()
                }
                other => {
                    return Err(GnosisError::Job(format!(
                        "cannot pause job {id} in status {other}"
                    )));
                }
            }
        };
        Self::remove_from_pending(&mut inner, &id_str);
        Ok(job)
    }

    fn unpause(&self, id: &JobId) -> Result<Job> {
        let mut inner = self.inner.lock().unwrap();
        let now = Utc::now();
        let (id_str, job) = {
            let job = inner
                .jobs
                .iter_mut()
                .find(|j| &j.id == id)
                .ok_or_else(|| GnosisError::Job(format!("missing job {id}")))?;
            if job.status != JobStatus::Paused {
                return Err(GnosisError::Job(format!(
                    "cannot unpause job {id} in status {}",
                    job.status
                )));
            }
            job.status = JobStatus::Pending;
            job.error = None;
            job.available_at = None;
            job.updated_at = now;
            (job.id.as_str().to_string(), job.clone())
        };
        Self::ensure_pending(&mut inner, &id_str);
        Ok(job)
    }

    fn stop(&self, id: &JobId) -> Result<Job> {
        let mut inner = self.inner.lock().unwrap();
        let now = Utc::now();
        let id_str = id.as_str().to_string();
        let job = {
            let job = inner
                .jobs
                .iter_mut()
                .find(|j| &j.id == id)
                .ok_or_else(|| GnosisError::Job(format!("missing job {id}")))?;
            match job.status {
                JobStatus::Pending | JobStatus::Paused | JobStatus::Running => {
                    job.status = JobStatus::Stopped;
                    job.error = Some("stopped by user".into());
                    job.result = None;
                    job.worker_id = None;
                    job.available_at = None;
                    job.finished_at = Some(now);
                    job.updated_at = now;
                    job.clone()
                }
                other => {
                    return Err(GnosisError::Job(format!(
                        "cannot stop job {id} in status {other}"
                    )));
                }
            }
        };
        Self::remove_from_pending(&mut inner, &id_str);
        Ok(job)
    }

    fn purge_scan(&self, scan_id: &str) -> Result<u64> {
        let mut inner = self.inner.lock().unwrap();
        let before = inner.jobs.len();
        let removed_ids: Vec<String> = inner
            .jobs
            .iter()
            .filter(|j| j.scan_id == scan_id)
            .map(|j| j.id.as_str().to_string())
            .collect();
        inner.jobs.retain(|j| j.scan_id != scan_id);
        inner
            .pending
            .retain(|id| !removed_ids.iter().any(|r| r == id));
        Ok((before - inner.jobs.len()) as u64)
    }

    fn purge_older_than(
        &self,
        older_than: chrono::DateTime<chrono::Utc>,
        scan_id: Option<&str>,
    ) -> Result<u64> {
        let mut inner = self.inner.lock().unwrap();
        let before = inner.jobs.len();
        let removed_ids: Vec<String> = inner
            .jobs
            .iter()
            .filter(|j| {
                j.updated_at < older_than && scan_id.map(|s| j.scan_id == s).unwrap_or(true)
            })
            .map(|j| j.id.as_str().to_string())
            .collect();
        let remove: std::collections::HashSet<_> = removed_ids.iter().cloned().collect();
        inner.jobs.retain(|j| !remove.contains(j.id.as_str()));
        inner.pending.retain(|id| !remove.contains(id));
        Ok((before - inner.jobs.len()) as u64)
    }
}
