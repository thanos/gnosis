//! Pause / unpause / stop helpers for jobs and scans.

use crate::error::{GnosisError, Result};
use crate::jobs::rerun::{resolve_job_id, resolve_scan_id};
use crate::jobs::store::JobStore;
use crate::jobs::types::{JobId, JobListFilter, JobStatus};

/// Apply `op` to each resolved job id query.
fn apply_queries(
    store: &dyn JobStore,
    queries: &[String],
    op: impl Fn(&dyn JobStore, &JobId) -> Result<JobId>,
) -> Result<Vec<JobId>> {
    let mut out = Vec::with_capacity(queries.len());
    for q in queries {
        let job = resolve_job_id(store, q)?;
        out.push(op(store, &job.id)?);
    }
    Ok(out)
}

/// Apply `op` to every job in `source_scan` whose status is in `eligible`.
fn apply_scan(
    store: &dyn JobStore,
    source_scan: &str,
    eligible: &[JobStatus],
    op: impl Fn(&dyn JobStore, &JobId) -> Result<JobId>,
) -> Result<Vec<JobId>> {
    let source = resolve_scan_id(store, source_scan)?;
    let jobs = store.list(&JobListFilter {
        scan_id: Some(source.clone()),
        status: None,
        limit: None,
    })?;
    let targets: Vec<_> = jobs
        .into_iter()
        .filter(|j| eligible.contains(&j.status))
        .collect();
    if targets.is_empty() {
        return Err(GnosisError::Job(format!(
            "no eligible jobs in scan {source} for this action"
        )));
    }
    let mut out = Vec::with_capacity(targets.len());
    for job in targets {
        out.push(op(store, &job.id)?);
    }
    Ok(out)
}

fn pause_one(store: &dyn JobStore, id: &JobId) -> Result<JobId> {
    Ok(store.pause(id)?.id)
}

fn unpause_one(store: &dyn JobStore, id: &JobId) -> Result<JobId> {
    Ok(store.unpause(id)?.id)
}

fn stop_one(store: &dyn JobStore, id: &JobId) -> Result<JobId> {
    Ok(store.stop(id)?.id)
}

/// Pause jobs by id query tokens.
pub fn pause_jobs(store: &dyn JobStore, queries: &[String]) -> Result<Vec<JobId>> {
    apply_queries(store, queries, pause_one)
}

/// Pause all pending/running jobs in a scan.
pub fn pause_scan(store: &dyn JobStore, source_scan: &str) -> Result<Vec<JobId>> {
    apply_scan(
        store,
        source_scan,
        &[JobStatus::Pending, JobStatus::Running],
        pause_one,
    )
}

/// Unpause jobs by id query tokens.
pub fn unpause_jobs(store: &dyn JobStore, queries: &[String]) -> Result<Vec<JobId>> {
    apply_queries(store, queries, unpause_one)
}

/// Unpause all paused jobs in a scan.
pub fn unpause_scan(store: &dyn JobStore, source_scan: &str) -> Result<Vec<JobId>> {
    apply_scan(store, source_scan, &[JobStatus::Paused], unpause_one)
}

/// Stop jobs by id query tokens.
pub fn stop_jobs(store: &dyn JobStore, queries: &[String]) -> Result<Vec<JobId>> {
    apply_queries(store, queries, stop_one)
}

/// Stop all pending/paused/running jobs in a scan.
pub fn stop_scan(store: &dyn JobStore, source_scan: &str) -> Result<Vec<JobId>> {
    apply_scan(
        store,
        source_scan,
        &[JobStatus::Pending, JobStatus::Paused, JobStatus::Running],
        stop_one,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{Job, JobStore, MemoryJobStore};

    #[test]
    fn pause_unpause_stop_lifecycle() {
        let store = MemoryJobStore::new();
        let id = store
            .enqueue(Job::new("scan:1", "analyze_object", serde_json::json!({})))
            .unwrap();

        let job = store.pause(&id).unwrap();
        assert_eq!(job.status, JobStatus::Paused);
        assert!(store.claim_next("scan:1", "w").unwrap().is_none());

        let job = store.unpause(&id).unwrap();
        assert_eq!(job.status, JobStatus::Pending);
        let claimed = store.claim_next("scan:1", "w").unwrap().unwrap();
        assert_eq!(claimed.id, id);

        let job = store.pause(&id).unwrap();
        assert_eq!(job.status, JobStatus::Paused);

        let job = store.stop(&id).unwrap();
        assert_eq!(job.status, JobStatus::Stopped);
        assert!(job.status.is_terminal());
        assert!(store.unpause(&id).is_err());
    }

    #[test]
    fn stop_scan_cancels_active() {
        let store = MemoryJobStore::new();
        store
            .enqueue(Job::new("scan:x", "k", serde_json::json!({})))
            .unwrap();
        store
            .enqueue(Job::new("scan:x", "k", serde_json::json!({})))
            .unwrap();
        let ids = stop_scan(&store, "x").unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(store.summary("scan:x").unwrap().stopped, 2);
    }
}
