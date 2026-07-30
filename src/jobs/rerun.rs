//! Re-queue and re-execute jobs by id or by scan.

use crate::error::{GnosisError, Result};
use crate::jobs::store::JobStore;
use crate::jobs::types::{Job, JobId, JobListFilter};
use std::sync::Arc;

/// Create a new scan id (`scan:{uuid}`).
pub fn new_scan_id() -> String {
    format!("scan:{}", uuid::Uuid::new_v4())
}

/// Create a rerun batch scan id (`rerun:{uuid}`).
pub fn new_rerun_scan_id() -> String {
    format!("rerun:{}", uuid::Uuid::new_v4())
}

/// Parse a comma-delimited list of job id tokens.
pub fn parse_job_id_list(input: &str) -> Result<Vec<String>> {
    let ids: Vec<String> = input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if ids.is_empty() {
        return Err(GnosisError::Job(
            "rerun requires at least one job id (comma-delimited)".into(),
        ));
    }
    Ok(ids)
}

/// Resolve a scan id token (full id, `scan:`/`rerun:` prefix, or unique substring).
pub fn resolve_scan_id(store: &dyn JobStore, query: &str) -> Result<String> {
    let ids = store.list_scan_ids()?;
    if ids.iter().any(|id| id == query) {
        return Ok(query.to_string());
    }
    for prefix in ["scan:", "rerun:"] {
        if !query.starts_with(prefix) {
            let candidate = format!("{prefix}{query}");
            if ids.iter().any(|id| id == &candidate) {
                return Ok(candidate);
            }
        }
    }
    let q = query.to_ascii_lowercase();
    let matches: Vec<_> = ids
        .into_iter()
        .filter(|id| {
            let idl = id.to_ascii_lowercase();
            idl == q || idl.ends_with(&q) || idl.contains(&q)
        })
        .collect();
    match matches.len() {
        0 => Err(GnosisError::Job(format!("scan not found: {query}"))),
        1 => Ok(matches.into_iter().next().unwrap()),
        n => Err(GnosisError::Job(format!(
            "ambiguous scan id '{query}' ({n} matches); use a longer prefix"
        ))),
    }
}

/// Resolve a job id token (full id, `job:` prefix, or unique substring) against a store.
pub fn resolve_job_id(store: &dyn JobStore, query: &str) -> Result<Job> {
    let id = JobId::new(query);
    if let Some(job) = store.get(&id)? {
        return Ok(job);
    }
    if !query.starts_with("job:") {
        let prefixed = JobId::new(format!("job:{query}"));
        if let Some(job) = store.get(&prefixed)? {
            return Ok(job);
        }
    }
    let all = store.list(&JobListFilter::default())?;
    let q = query.to_ascii_lowercase();
    let matches: Vec<_> = all
        .into_iter()
        .filter(|j| {
            let id = j.id.as_str().to_ascii_lowercase();
            id == q || id.ends_with(&q) || id.contains(&q)
        })
        .collect();
    match matches.len() {
        0 => Err(GnosisError::Job(format!("job not found: {query}"))),
        1 => Ok(matches.into_iter().next().unwrap()),
        n => Err(GnosisError::Job(format!(
            "ambiguous job id '{query}' ({n} matches); use a longer prefix"
        ))),
    }
}

/// Requeue each resolved job under `scan_id` for another run.
pub fn requeue_jobs(store: &dyn JobStore, queries: &[String], scan_id: &str) -> Result<Vec<JobId>> {
    let mut out = Vec::with_capacity(queries.len());
    for q in queries {
        let job = resolve_job_id(store, q)?;
        let job = store.requeue(&job.id, Some(scan_id))?;
        out.push(job.id);
    }
    Ok(out)
}

/// Requeue every job currently linked to `source_scan_id` under a new rerun scan.
pub fn requeue_scan(
    store: &dyn JobStore,
    source_scan_id: &str,
    new_scan_id: &str,
) -> Result<Vec<JobId>> {
    let jobs = store.list(&JobListFilter {
        scan_id: Some(source_scan_id.to_string()),
        status: None,
        limit: None,
    })?;
    if jobs.is_empty() {
        return Err(GnosisError::Job(format!(
            "no jobs found for scan {source_scan_id}"
        )));
    }
    let mut out = Vec::with_capacity(jobs.len());
    for job in jobs {
        let job = store.requeue(&job.id, Some(new_scan_id))?;
        out.push(job.id);
    }
    Ok(out)
}

/// Summary of a rerun batch.
#[derive(Clone, Debug)]
pub struct RerunReport {
    pub scan_id: String,
    pub requested: usize,
    pub requeued: Vec<JobId>,
}

impl RerunReport {
    pub fn format_summary(&self, completed: u64, failed: u64) -> String {
        format!(
            "rerun scan {} — requeued {} job(s); completed={} failed={}",
            self.scan_id,
            self.requeued.len(),
            completed,
            failed
        )
    }
}

/// Requeue jobs by query tokens under a fresh rerun scan id (does not execute).
pub fn prepare_rerun(store: Arc<dyn JobStore>, queries: &[String]) -> Result<RerunReport> {
    let scan_id = new_rerun_scan_id();
    let requeued = requeue_jobs(store.as_ref(), queries, &scan_id)?;
    Ok(RerunReport {
        scan_id,
        requested: queries.len(),
        requeued,
    })
}

/// Requeue all jobs from an existing scan under a fresh rerun scan id (does not execute).
pub fn prepare_rerun_scan(store: Arc<dyn JobStore>, source_scan: &str) -> Result<RerunReport> {
    let source = resolve_scan_id(store.as_ref(), source_scan)?;
    let scan_id = new_rerun_scan_id();
    let requeued = requeue_scan(store.as_ref(), &source, &scan_id)?;
    Ok(RerunReport {
        scan_id,
        requested: requeued.len(),
        requeued,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{Job, JobStatus, MemoryJobStore};

    #[test]
    fn parse_comma_list() {
        let ids = parse_job_id_list(" a , b,c ").unwrap();
        assert_eq!(ids, vec!["a", "b", "c"]);
        assert!(parse_job_id_list(" , , ").is_err());
    }

    #[test]
    fn requeue_resets_status() {
        let store = MemoryJobStore::new();
        store
            .enqueue(Job::new("scan:1", "analyze_object", serde_json::json!({})))
            .unwrap();
        let claimed = store.claim_next("scan:1", "w").unwrap().unwrap();
        let id = claimed.id.clone();
        store
            .complete(&id, serde_json::json!({"ok": true}))
            .unwrap();
        let job = store.requeue(&id, Some("rerun:1")).unwrap();
        assert_eq!(job.status, JobStatus::Pending);
        assert_eq!(job.scan_id, "rerun:1");
        assert!(job.result.is_none());
        let claimed = store.claim_next("rerun:1", "w").unwrap().unwrap();
        assert_eq!(claimed.id, id);
    }

    #[test]
    fn prepare_rerun_scan_moves_jobs() {
        let store = Arc::new(MemoryJobStore::new()) as Arc<dyn JobStore>;
        let a = store
            .enqueue(Job::new(
                "scan:abc",
                "analyze_object",
                serde_json::json!({}),
            ))
            .unwrap();
        store
            .enqueue(Job::new(
                "scan:abc",
                "analyze_object",
                serde_json::json!({}),
            ))
            .unwrap();
        let _ = store.claim_next("scan:abc", "w").unwrap().unwrap();
        store.complete(&a, serde_json::json!({})).unwrap();

        let report = prepare_rerun_scan(Arc::clone(&store), "abc").unwrap();
        assert_eq!(report.requeued.len(), 2);
        assert!(report.scan_id.starts_with("rerun:"));
        assert_eq!(store.summary("scan:abc").unwrap().total(), 0);
        assert_eq!(store.summary(&report.scan_id).unwrap().pending, 2);
    }
}
