use crate::error::{GnosisError, Result};
use crate::jobs::store::JobStore;
use crate::jobs::types::{Job, JobId, JobStatus, JobSummary};
use chrono::Utc;
use redb::{Database, ReadableTable, TableDefinition};
use std::path::Path;
use std::sync::Mutex;

const JOBS: TableDefinition<&str, &[u8]> = TableDefinition::new("jobs");
/// FIFO pending queue: monotonic sequence → job id.
const PENDING: TableDefinition<u64, &str> = TableDefinition::new("pending");
const META: TableDefinition<&str, u64> = TableDefinition::new("meta");

const META_NEXT_SEQ: &str = "next_seq";

/// Default durable [`JobStore`] backed by [redb](https://docs.rs/redb).
pub struct RedbJobStore {
    db: Database,
    /// Serialize writers; redb allows one write txn at a time anyway.
    write_lock: Mutex<()>,
}

impl RedbJobStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let db = Database::create(path)
            .map_err(|e| GnosisError::Job(format!("open redb {}: {e}", path.display())))?;
        let store = Self {
            db,
            write_lock: Mutex::new(()),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// In-memory redb database (useful for tests).
    pub fn in_memory() -> Result<Self> {
        let db = Database::builder()
            .create_with_backend(redb::backends::InMemoryBackend::new())
            .map_err(|e| GnosisError::Job(format!("open in-memory redb: {e}")))?;
        let store = Self {
            db,
            write_lock: Mutex::new(()),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        let _guard = self.write_lock.lock().unwrap();
        let txn = self
            .db
            .begin_write()
            .map_err(|e| GnosisError::Job(format!("redb begin_write: {e}")))?;
        {
            let _jobs = txn
                .open_table(JOBS)
                .map_err(|e| GnosisError::Job(format!("open jobs table: {e}")))?;
            let _pending = txn
                .open_table(PENDING)
                .map_err(|e| GnosisError::Job(format!("open pending table: {e}")))?;
            let mut meta = txn
                .open_table(META)
                .map_err(|e| GnosisError::Job(format!("open meta table: {e}")))?;
            if meta
                .get(META_NEXT_SEQ)
                .map_err(|e| GnosisError::Job(format!("meta get: {e}")))?
                .is_none()
            {
                meta.insert(META_NEXT_SEQ, 1u64)
                    .map_err(|e| GnosisError::Job(format!("meta insert: {e}")))?;
            }
        }
        txn.commit()
            .map_err(|e| GnosisError::Job(format!("redb commit: {e}")))?;
        Ok(())
    }

    fn encode(job: &Job) -> Result<Vec<u8>> {
        serde_json::to_vec(job).map_err(|e| GnosisError::Job(format!("encode job: {e}")))
    }

    fn decode(bytes: &[u8]) -> Result<Job> {
        serde_json::from_slice(bytes).map_err(|e| GnosisError::Job(format!("decode job: {e}")))
    }

    fn update_job<F>(&self, id: &JobId, f: F) -> Result<()>
    where
        F: FnOnce(&mut Job) -> Result<()>,
    {
        let _guard = self.write_lock.lock().unwrap();
        let txn = self
            .db
            .begin_write()
            .map_err(|e| GnosisError::Job(format!("redb begin_write: {e}")))?;
        {
            let mut table = txn
                .open_table(JOBS)
                .map_err(|e| GnosisError::Job(format!("open jobs: {e}")))?;
            let existing = table
                .get(id.as_str())
                .map_err(|e| GnosisError::Job(format!("jobs get: {e}")))?
                .ok_or_else(|| GnosisError::Job(format!("missing job {id}")))?;
            let mut job = Self::decode(existing.value())?;
            drop(existing);
            f(&mut job)?;
            let bytes = Self::encode(&job)?;
            table
                .insert(id.as_str(), bytes.as_slice())
                .map_err(|e| GnosisError::Job(format!("jobs insert: {e}")))?;
        }
        txn.commit()
            .map_err(|e| GnosisError::Job(format!("redb commit: {e}")))?;
        Ok(())
    }

    fn control_job(&self, id: &JobId, action: ControlAction) -> Result<Job> {
        let _guard = self.write_lock.lock().unwrap();
        let txn = self
            .db
            .begin_write()
            .map_err(|e| GnosisError::Job(format!("redb begin_write: {e}")))?;
        let job = {
            let mut jobs = txn
                .open_table(JOBS)
                .map_err(|e| GnosisError::Job(format!("open jobs: {e}")))?;
            let mut pending = txn
                .open_table(PENDING)
                .map_err(|e| GnosisError::Job(format!("open pending: {e}")))?;
            let mut meta = txn
                .open_table(META)
                .map_err(|e| GnosisError::Job(format!("open meta: {e}")))?;

            let existing = jobs
                .get(id.as_str())
                .map_err(|e| GnosisError::Job(format!("jobs get: {e}")))?
                .ok_or_else(|| GnosisError::Job(format!("missing job {id}")))?;
            let mut job = Self::decode(existing.value())?;
            drop(existing);

            let now = Utc::now();
            match action {
                ControlAction::Pause => match job.status {
                    JobStatus::Pending | JobStatus::Running => {
                        job.status = JobStatus::Paused;
                        job.worker_id = None;
                        job.available_at = None;
                        job.updated_at = now;
                    }
                    other => {
                        return Err(GnosisError::Job(format!(
                            "cannot pause job {id} in status {other}"
                        )));
                    }
                },
                ControlAction::Unpause => {
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
                }
                ControlAction::Stop => match job.status {
                    JobStatus::Pending | JobStatus::Paused | JobStatus::Running => {
                        job.status = JobStatus::Stopped;
                        job.error = Some("stopped by user".into());
                        job.result = None;
                        job.worker_id = None;
                        job.available_at = None;
                        job.finished_at = Some(now);
                        job.updated_at = now;
                    }
                    other => {
                        return Err(GnosisError::Job(format!(
                            "cannot stop job {id} in status {other}"
                        )));
                    }
                },
            }

            let bytes = Self::encode(&job)?;
            jobs.insert(id.as_str(), bytes.as_slice())
                .map_err(|e| GnosisError::Job(format!("jobs insert: {e}")))?;

            // Sync pending FIFO with the new status.
            Self::remove_pending_id(&mut pending, id.as_str())?;
            if job.status == JobStatus::Pending {
                let seq = meta
                    .get(META_NEXT_SEQ)
                    .map_err(|e| GnosisError::Job(format!("meta get: {e}")))?
                    .map(|v| v.value())
                    .unwrap_or(1);
                meta.insert(META_NEXT_SEQ, seq + 1)
                    .map_err(|e| GnosisError::Job(format!("meta insert: {e}")))?;
                pending
                    .insert(seq, id.as_str())
                    .map_err(|e| GnosisError::Job(format!("pending insert: {e}")))?;
            }
            job
        };
        txn.commit()
            .map_err(|e| GnosisError::Job(format!("redb commit: {e}")))?;
        Ok(job)
    }

    fn remove_pending_id(pending: &mut redb::Table<'_, u64, &str>, job_id: &str) -> Result<()> {
        let mut remove: Vec<u64> = Vec::new();
        {
            let iter = pending
                .iter()
                .map_err(|e| GnosisError::Job(format!("pending iter: {e}")))?;
            for item in iter {
                let (k, v) = item.map_err(|e| GnosisError::Job(format!("pending item: {e}")))?;
                if v.value() == job_id {
                    remove.push(k.value());
                }
            }
        }
        for seq in remove {
            pending
                .remove(seq)
                .map_err(|e| GnosisError::Job(format!("pending remove: {e}")))?;
        }
        Ok(())
    }
}

enum ControlAction {
    Pause,
    Unpause,
    Stop,
}

impl JobStore for RedbJobStore {
    fn enqueue(&self, job: Job) -> Result<JobId> {
        let _guard = self.write_lock.lock().unwrap();
        let id = job.id.clone();
        let bytes = Self::encode(&job)?;
        let txn = self
            .db
            .begin_write()
            .map_err(|e| GnosisError::Job(format!("redb begin_write: {e}")))?;
        {
            let mut jobs = txn
                .open_table(JOBS)
                .map_err(|e| GnosisError::Job(format!("open jobs: {e}")))?;
            let mut pending = txn
                .open_table(PENDING)
                .map_err(|e| GnosisError::Job(format!("open pending: {e}")))?;
            let mut meta = txn
                .open_table(META)
                .map_err(|e| GnosisError::Job(format!("open meta: {e}")))?;

            let seq = meta
                .get(META_NEXT_SEQ)
                .map_err(|e| GnosisError::Job(format!("meta get: {e}")))?
                .map(|v| v.value())
                .unwrap_or(1);
            meta.insert(META_NEXT_SEQ, seq + 1)
                .map_err(|e| GnosisError::Job(format!("meta insert: {e}")))?;

            jobs.insert(id.as_str(), bytes.as_slice())
                .map_err(|e| GnosisError::Job(format!("jobs insert: {e}")))?;
            pending
                .insert(seq, id.as_str())
                .map_err(|e| GnosisError::Job(format!("pending insert: {e}")))?;
        }
        txn.commit()
            .map_err(|e| GnosisError::Job(format!("redb commit: {e}")))?;
        Ok(id)
    }

    fn claim_next(&self, scan_id: &str, worker_id: &str) -> Result<Option<Job>> {
        let _guard = self.write_lock.lock().unwrap();
        let txn = self
            .db
            .begin_write()
            .map_err(|e| GnosisError::Job(format!("redb begin_write: {e}")))?;

        let claimed = {
            let mut pending = txn
                .open_table(PENDING)
                .map_err(|e| GnosisError::Job(format!("open pending: {e}")))?;
            let mut jobs = txn
                .open_table(JOBS)
                .map_err(|e| GnosisError::Job(format!("open jobs: {e}")))?;

            // Walk FIFO until we find a pending job for this scan.
            let mut chosen: Option<(u64, String)> = None;
            {
                let iter = pending
                    .iter()
                    .map_err(|e| GnosisError::Job(format!("pending iter: {e}")))?;
                for item in iter {
                    let (k, v) =
                        item.map_err(|e| GnosisError::Job(format!("pending item: {e}")))?;
                    let seq = k.value();
                    let job_id = v.value().to_string();
                    let Some(bytes) = jobs
                        .get(job_id.as_str())
                        .map_err(|e| GnosisError::Job(format!("jobs get: {e}")))?
                    else {
                        continue;
                    };
                    let job = Self::decode(bytes.value())?;
                    if job.scan_id == scan_id && job.is_claimable_at(Utc::now()) {
                        chosen = Some((seq, job_id));
                        break;
                    }
                }
            }

            if let Some((seq, job_id)) = chosen {
                pending
                    .remove(seq)
                    .map_err(|e| GnosisError::Job(format!("pending remove: {e}")))?;
                let existing = jobs
                    .get(job_id.as_str())
                    .map_err(|e| GnosisError::Job(format!("jobs get: {e}")))?
                    .ok_or_else(|| GnosisError::Job(format!("missing job {job_id}")))?;
                let mut job = Self::decode(existing.value())?;
                drop(existing);
                let now = Utc::now();
                job.status = JobStatus::Running;
                job.attempts += 1;
                job.started_at = Some(now);
                job.updated_at = now;
                job.available_at = None;
                job.worker_id = Some(worker_id.to_string());
                let bytes = Self::encode(&job)?;
                jobs.insert(job_id.as_str(), bytes.as_slice())
                    .map_err(|e| GnosisError::Job(format!("jobs insert: {e}")))?;
                Some(job)
            } else {
                None
            }
        };

        txn.commit()
            .map_err(|e| GnosisError::Job(format!("redb commit: {e}")))?;
        Ok(claimed)
    }

    fn complete(&self, id: &JobId, result: serde_json::Value) -> Result<()> {
        self.update_job(id, |job| {
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
        })
    }

    fn fail(&self, id: &JobId, error: String) -> Result<()> {
        self.update_job(id, |job| {
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
        })
    }

    fn schedule_retry(
        &self,
        id: &JobId,
        error: String,
        available_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        let _guard = self.write_lock.lock().unwrap();
        let txn = self
            .db
            .begin_write()
            .map_err(|e| GnosisError::Job(format!("redb begin_write: {e}")))?;
        {
            let mut jobs = txn
                .open_table(JOBS)
                .map_err(|e| GnosisError::Job(format!("open jobs: {e}")))?;
            let mut pending = txn
                .open_table(PENDING)
                .map_err(|e| GnosisError::Job(format!("open pending: {e}")))?;
            let mut meta = txn
                .open_table(META)
                .map_err(|e| GnosisError::Job(format!("open meta: {e}")))?;

            let existing = jobs
                .get(id.as_str())
                .map_err(|e| GnosisError::Job(format!("jobs get: {e}")))?
                .ok_or_else(|| GnosisError::Job(format!("missing job {id}")))?;
            let mut job = Self::decode(existing.value())?;
            drop(existing);
            if job.status != JobStatus::Running {
                // Job was paused/stopped while executing — leave it alone.
            } else {
                let now = Utc::now();
                job.status = JobStatus::Pending;
                job.error = Some(error);
                job.result = None;
                job.worker_id = None;
                job.finished_at = None;
                job.available_at = Some(available_at);
                job.updated_at = now;
                let bytes = Self::encode(&job)?;
                jobs.insert(id.as_str(), bytes.as_slice())
                    .map_err(|e| GnosisError::Job(format!("jobs insert: {e}")))?;

                Self::remove_pending_id(&mut pending, id.as_str())?;
                let seq = meta
                    .get(META_NEXT_SEQ)
                    .map_err(|e| GnosisError::Job(format!("meta get: {e}")))?
                    .map(|v| v.value())
                    .unwrap_or(1);
                meta.insert(META_NEXT_SEQ, seq + 1)
                    .map_err(|e| GnosisError::Job(format!("meta insert: {e}")))?;
                pending
                    .insert(seq, id.as_str())
                    .map_err(|e| GnosisError::Job(format!("pending insert: {e}")))?;
            }
        }
        txn.commit()
            .map_err(|e| GnosisError::Job(format!("redb commit: {e}")))?;
        Ok(())
    }

    fn get(&self, id: &JobId) -> Result<Option<Job>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| GnosisError::Job(format!("redb begin_read: {e}")))?;
        let table = txn
            .open_table(JOBS)
            .map_err(|e| GnosisError::Job(format!("open jobs: {e}")))?;
        match table
            .get(id.as_str())
            .map_err(|e| GnosisError::Job(format!("jobs get: {e}")))?
        {
            Some(v) => Ok(Some(Self::decode(v.value())?)),
            None => Ok(None),
        }
    }

    fn list(&self, filter: &crate::jobs::types::JobListFilter) -> Result<Vec<Job>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| GnosisError::Job(format!("redb begin_read: {e}")))?;
        let table = txn
            .open_table(JOBS)
            .map_err(|e| GnosisError::Job(format!("open jobs: {e}")))?;
        let mut jobs = Vec::new();
        let iter = table
            .iter()
            .map_err(|e| GnosisError::Job(format!("jobs iter: {e}")))?;
        for item in iter {
            let (_, v) = item.map_err(|e| GnosisError::Job(format!("jobs item: {e}")))?;
            let job = Self::decode(v.value())?;
            if let Some(scan) = &filter.scan_id {
                if job.scan_id != *scan {
                    continue;
                }
            }
            if let Some(status) = filter.status {
                if job.status != status {
                    continue;
                }
            }
            jobs.push(job);
        }
        jobs.sort_by_key(|b| std::cmp::Reverse(b.updated_at));
        if let Some(limit) = filter.limit {
            jobs.truncate(limit);
        }
        Ok(jobs)
    }

    fn summary(&self, scan_id: &str) -> Result<JobSummary> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| GnosisError::Job(format!("redb begin_read: {e}")))?;
        let table = txn
            .open_table(JOBS)
            .map_err(|e| GnosisError::Job(format!("open jobs: {e}")))?;
        let mut s = JobSummary::default();
        let iter = table
            .iter()
            .map_err(|e| GnosisError::Job(format!("jobs iter: {e}")))?;
        for item in iter {
            let (_, v) = item.map_err(|e| GnosisError::Job(format!("jobs item: {e}")))?;
            let job = Self::decode(v.value())?;
            if job.scan_id != scan_id {
                continue;
            }
            s.record(job.status);
        }
        Ok(s)
    }

    fn reclaim_stale(&self, scan_id: &str) -> Result<u64> {
        let _guard = self.write_lock.lock().unwrap();
        let txn = self
            .db
            .begin_write()
            .map_err(|e| GnosisError::Job(format!("redb begin_write: {e}")))?;
        let mut n = 0u64;
        {
            let mut jobs = txn
                .open_table(JOBS)
                .map_err(|e| GnosisError::Job(format!("open jobs: {e}")))?;
            let mut pending = txn
                .open_table(PENDING)
                .map_err(|e| GnosisError::Job(format!("open pending: {e}")))?;
            let mut meta = txn
                .open_table(META)
                .map_err(|e| GnosisError::Job(format!("open meta: {e}")))?;

            let mut to_requeue: Vec<Job> = Vec::new();
            {
                let iter = jobs
                    .iter()
                    .map_err(|e| GnosisError::Job(format!("jobs iter: {e}")))?;
                for item in iter {
                    let (_, v) = item.map_err(|e| GnosisError::Job(format!("jobs item: {e}")))?;
                    let job = Self::decode(v.value())?;
                    if job.scan_id == scan_id && job.status == JobStatus::Running {
                        to_requeue.push(job);
                    }
                }
            }

            for mut job in to_requeue {
                let now = Utc::now();
                job.status = JobStatus::Pending;
                job.worker_id = None;
                job.started_at = None;
                job.updated_at = now;
                let bytes = Self::encode(&job)?;
                jobs.insert(job.id.as_str(), bytes.as_slice())
                    .map_err(|e| GnosisError::Job(format!("jobs insert: {e}")))?;

                let seq = meta
                    .get(META_NEXT_SEQ)
                    .map_err(|e| GnosisError::Job(format!("meta get: {e}")))?
                    .map(|v| v.value())
                    .unwrap_or(1);
                meta.insert(META_NEXT_SEQ, seq + 1)
                    .map_err(|e| GnosisError::Job(format!("meta insert: {e}")))?;
                pending
                    .insert(seq, job.id.as_str())
                    .map_err(|e| GnosisError::Job(format!("pending insert: {e}")))?;
                n += 1;
            }
        }
        txn.commit()
            .map_err(|e| GnosisError::Job(format!("redb commit: {e}")))?;
        Ok(n)
    }

    fn requeue(&self, id: &JobId, new_scan_id: Option<&str>) -> Result<Job> {
        let _guard = self.write_lock.lock().unwrap();
        let txn = self
            .db
            .begin_write()
            .map_err(|e| GnosisError::Job(format!("redb begin_write: {e}")))?;
        let job = {
            let mut jobs = txn
                .open_table(JOBS)
                .map_err(|e| GnosisError::Job(format!("open jobs: {e}")))?;
            let mut pending = txn
                .open_table(PENDING)
                .map_err(|e| GnosisError::Job(format!("open pending: {e}")))?;
            let mut meta = txn
                .open_table(META)
                .map_err(|e| GnosisError::Job(format!("open meta: {e}")))?;

            let existing = jobs
                .get(id.as_str())
                .map_err(|e| GnosisError::Job(format!("jobs get: {e}")))?
                .ok_or_else(|| GnosisError::Job(format!("missing job {id}")))?;
            let mut job = Self::decode(existing.value())?;
            drop(existing);

            let now = Utc::now();
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

            let bytes = Self::encode(&job)?;
            jobs.insert(id.as_str(), bytes.as_slice())
                .map_err(|e| GnosisError::Job(format!("jobs insert: {e}")))?;

            // Ensure on pending FIFO (skip if already queued).
            let already = {
                let mut found = false;
                let iter = pending
                    .iter()
                    .map_err(|e| GnosisError::Job(format!("pending iter: {e}")))?;
                for item in iter {
                    let (_, v) =
                        item.map_err(|e| GnosisError::Job(format!("pending item: {e}")))?;
                    if v.value() == id.as_str() {
                        found = true;
                        break;
                    }
                }
                found
            };
            if !already {
                let seq = meta
                    .get(META_NEXT_SEQ)
                    .map_err(|e| GnosisError::Job(format!("meta get: {e}")))?
                    .map(|v| v.value())
                    .unwrap_or(1);
                meta.insert(META_NEXT_SEQ, seq + 1)
                    .map_err(|e| GnosisError::Job(format!("meta insert: {e}")))?;
                pending
                    .insert(seq, id.as_str())
                    .map_err(|e| GnosisError::Job(format!("pending insert: {e}")))?;
            }
            job
        };
        txn.commit()
            .map_err(|e| GnosisError::Job(format!("redb commit: {e}")))?;
        Ok(job)
    }

    fn pause(&self, id: &JobId) -> Result<Job> {
        self.control_job(id, ControlAction::Pause)
    }

    fn unpause(&self, id: &JobId) -> Result<Job> {
        self.control_job(id, ControlAction::Unpause)
    }

    fn stop(&self, id: &JobId) -> Result<Job> {
        self.control_job(id, ControlAction::Stop)
    }

    fn purge_scan(&self, scan_id: &str) -> Result<u64> {
        self.delete_jobs(|job| job.scan_id == scan_id)
    }

    fn purge_older_than(
        &self,
        older_than: chrono::DateTime<chrono::Utc>,
        scan_id: Option<&str>,
    ) -> Result<u64> {
        self.delete_jobs(|job| {
            job.updated_at < older_than && scan_id.map(|s| job.scan_id == s).unwrap_or(true)
        })
    }
}

impl RedbJobStore {
    fn delete_jobs(&self, pred: impl Fn(&Job) -> bool) -> Result<u64> {
        let _guard = self.write_lock.lock().unwrap();
        let txn = self
            .db
            .begin_write()
            .map_err(|e| GnosisError::Job(format!("redb begin_write: {e}")))?;
        let mut n = 0u64;
        {
            let mut jobs = txn
                .open_table(JOBS)
                .map_err(|e| GnosisError::Job(format!("open jobs: {e}")))?;
            let mut pending = txn
                .open_table(PENDING)
                .map_err(|e| GnosisError::Job(format!("open pending: {e}")))?;

            let mut to_delete: Vec<String> = Vec::new();
            {
                let iter = jobs
                    .iter()
                    .map_err(|e| GnosisError::Job(format!("jobs iter: {e}")))?;
                for item in iter {
                    let (k, v) = item.map_err(|e| GnosisError::Job(format!("jobs item: {e}")))?;
                    let job = Self::decode(v.value())?;
                    if pred(&job) {
                        to_delete.push(k.value().to_string());
                    }
                }
            }

            for id in &to_delete {
                jobs.remove(id.as_str())
                    .map_err(|e| GnosisError::Job(format!("jobs remove: {e}")))?;
                n += 1;
            }

            let mut pending_remove: Vec<u64> = Vec::new();
            {
                let iter = pending
                    .iter()
                    .map_err(|e| GnosisError::Job(format!("pending iter: {e}")))?;
                for item in iter {
                    let (k, v) =
                        item.map_err(|e| GnosisError::Job(format!("pending item: {e}")))?;
                    if to_delete.iter().any(|id| id == v.value()) {
                        pending_remove.push(k.value());
                    }
                }
            }
            for seq in pending_remove {
                pending
                    .remove(seq)
                    .map_err(|e| GnosisError::Job(format!("pending remove: {e}")))?;
            }
        }
        txn.commit()
            .map_err(|e| GnosisError::Job(format!("redb commit: {e}")))?;
        Ok(n)
    }
}
