use gnosis::{
    format_job_detail, format_job_list, Job, JobListFilter, JobStore, JobWorkerPool,
    MemoryJobStore, RedbJobStore, KIND_ANALYZE_OBJECT,
};
use gnosis::{JobExecutor, JobStatus, RetryPolicy};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

struct CountingExecutor {
    hits: Arc<AtomicUsize>,
    fail_once: Arc<AtomicBool>,
}

impl JobExecutor for CountingExecutor {
    fn execute(&self, job: &Job) -> gnosis::Result<serde_json::Value> {
        self.hits.fetch_add(1, Ordering::Relaxed);
        if self.fail_once.swap(false, Ordering::Relaxed) {
            return Err(gnosis::GnosisError::Job("boom".into()));
        }
        Ok(serde_json::json!({
            "kind": job.kind,
            "echo": job.args,
        }))
    }
}

fn run_store_roundtrip(store: Arc<dyn JobStore>) {
    let scan = "scan-test";
    let job = Job::new(
        scan,
        KIND_ANALYZE_OBJECT,
        serde_json::json!({"path": "a.rs"}),
    );
    let id = store.enqueue(job).unwrap();
    let loaded = store.get(&id).unwrap().unwrap();
    assert_eq!(loaded.status, JobStatus::Pending);
    assert_eq!(loaded.kind, KIND_ANALYZE_OBJECT);

    let claimed = store.claim_next(scan, "w1").unwrap().unwrap();
    assert_eq!(claimed.id, id);
    assert_eq!(claimed.status, JobStatus::Running);
    assert_eq!(claimed.attempts, 1);

    store
        .complete(&id, serde_json::json!({"ok": true}))
        .unwrap();
    let done = store.get(&id).unwrap().unwrap();
    assert_eq!(done.status, JobStatus::Completed);
    assert_eq!(done.result.unwrap()["ok"], true);

    let summary = store.summary(scan).unwrap();
    assert_eq!(summary.completed, 1);
    assert_eq!(summary.pending, 0);
}

#[test]
fn memory_job_store_roundtrip() {
    run_store_roundtrip(Arc::new(MemoryJobStore::new()));
}

#[test]
fn redb_job_store_roundtrip() {
    let store = Arc::new(RedbJobStore::in_memory().unwrap());
    run_store_roundtrip(store);
}

#[test]
fn redb_job_store_persists_to_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("jobs.redb");
    let scan = "persist";
    {
        let store = RedbJobStore::open(&path).unwrap();
        let job = Job::new(scan, "demo", serde_json::json!({"n": 1}));
        let id = store.enqueue(job).unwrap();
        let _ = store.claim_next(scan, "w").unwrap().unwrap();
        store
            .complete(&id, serde_json::json!({"done": true}))
            .unwrap();
    }
    let store = RedbJobStore::open(&path).unwrap();
    let summary = store.summary(scan).unwrap();
    assert_eq!(summary.completed, 1);
}

#[test]
fn reclaim_stale_running_jobs() {
    let store = Arc::new(MemoryJobStore::new());
    let scan = "reclaim";
    let id = store
        .enqueue(Job::new(scan, "x", serde_json::json!({})))
        .unwrap();
    let _ = store.claim_next(scan, "w").unwrap().unwrap();
    assert_eq!(store.summary(scan).unwrap().running, 1);
    let n = store.reclaim_stale(scan).unwrap();
    assert_eq!(n, 1);
    assert_eq!(store.summary(scan).unwrap().pending, 1);
    let again = store.claim_next(scan, "w2").unwrap().unwrap();
    assert_eq!(again.id, id);
}

#[test]
fn async_worker_pool_drains_queue() {
    let store: Arc<dyn JobStore> = Arc::new(MemoryJobStore::new());
    let scan = "pool";
    for i in 0..5 {
        store
            .enqueue(Job::new(scan, "echo", serde_json::json!({"i": i})))
            .unwrap();
    }

    let hits = Arc::new(AtomicUsize::new(0));
    let executor: Arc<dyn JobExecutor> = Arc::new(CountingExecutor {
        hits: Arc::clone(&hits),
        fail_once: Arc::new(AtomicBool::new(true)),
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let pool = JobWorkerPool::with_retry(
        Arc::clone(&store),
        executor,
        scan,
        cancel,
        2,
        Duration::from_millis(5),
        RetryPolicy::new(1, 1, 1),
    );
    pool.mark_enqueue_done();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(pool.run()).unwrap();

    assert_eq!(hits.load(Ordering::Relaxed), 5);
    let summary = store.summary(scan).unwrap();
    assert_eq!(summary.completed, 4);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.active(), 0);
}

#[test]
fn worker_pool_auto_retries_transient_failure() {
    let store: Arc<dyn JobStore> = Arc::new(MemoryJobStore::new());
    let scan = "retry";
    store
        .enqueue(Job::new(scan, "echo", serde_json::json!({"i": 0})))
        .unwrap();

    let hits = Arc::new(AtomicUsize::new(0));
    let executor: Arc<dyn JobExecutor> = Arc::new(CountingExecutor {
        hits: Arc::clone(&hits),
        fail_once: Arc::new(AtomicBool::new(true)),
    });
    let pool = JobWorkerPool::with_retry(
        Arc::clone(&store),
        executor,
        scan,
        Arc::new(AtomicBool::new(false)),
        1,
        Duration::from_millis(5),
        RetryPolicy::new(3, 5, 20),
    );
    pool.mark_enqueue_done();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(pool.run()).unwrap();

    // First attempt fails, backoff requeues it, second attempt succeeds.
    assert_eq!(hits.load(Ordering::Relaxed), 2);
    let summary = store.summary(scan).unwrap();
    assert_eq!(summary.completed, 1);
    assert_eq!(summary.failed, 0);
}

#[test]
fn worker_pool_fails_after_max_attempts() {
    let store: Arc<dyn JobStore> = Arc::new(MemoryJobStore::new());
    let scan = "retry-exhausted";
    let id = store
        .enqueue(Job::new(scan, "always-fails", serde_json::json!({})))
        .unwrap();

    struct AlwaysFails;
    impl JobExecutor for AlwaysFails {
        fn execute(&self, _job: &Job) -> gnosis::Result<serde_json::Value> {
            Err(gnosis::GnosisError::Job("nope".into()))
        }
    }

    let pool = JobWorkerPool::with_retry(
        Arc::clone(&store),
        Arc::new(AlwaysFails),
        scan,
        Arc::new(AtomicBool::new(false)),
        1,
        Duration::from_millis(5),
        RetryPolicy::new(3, 5, 20),
    );
    pool.mark_enqueue_done();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(pool.run()).unwrap();

    let job = store.get(&id).unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Failed);
    assert_eq!(job.attempts, 3);
    assert!(job.error.unwrap().contains("nope"));
}

#[test]
fn retry_backoff_delays_claim() {
    use chrono::{Duration as ChronoDuration, Utc};

    let store = MemoryJobStore::new();
    let scan = "backoff";
    let id = store
        .enqueue(Job::new(scan, "k", serde_json::json!({})))
        .unwrap();
    let _ = store.claim_next(scan, "w").unwrap().unwrap();

    store
        .schedule_retry(&id, "boom".into(), Utc::now() + ChronoDuration::seconds(60))
        .unwrap();

    let job = store.get(&id).unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Pending);
    assert!(job.available_at.is_some());
    // Still pending, but not claimable until the backoff elapses.
    assert!(store.claim_next(scan, "w").unwrap().is_none());

    // A job that is no longer running is left alone.
    store
        .schedule_retry(&id, "ignored".into(), Utc::now())
        .unwrap();
    assert_eq!(store.get(&id).unwrap().unwrap().error.unwrap(), "boom");
}

#[test]
fn list_filters_by_status_and_formats_views() {
    let store = Arc::new(MemoryJobStore::new());
    let a = store
        .enqueue(Job::new(
            "s1",
            KIND_ANALYZE_OBJECT,
            serde_json::json!({"relative_path": "a.rs"}),
        ))
        .unwrap();
    let b = store
        .enqueue(Job::new(
            "s1",
            KIND_ANALYZE_OBJECT,
            serde_json::json!({"relative_path": "b.rs"}),
        ))
        .unwrap();
    let claimed_a = store.claim_next("s1", "w").unwrap().unwrap();
    assert_eq!(claimed_a.id, a);
    store
        .complete(&a, serde_json::json!({"status": "understood"}))
        .unwrap();
    let _ = store.claim_next("s1", "w").unwrap();

    let completed = store
        .list(&JobListFilter {
            status: Some(JobStatus::Completed),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(completed.len(), 1);
    assert_eq!(completed[0].id, a);

    let running = store
        .list(&JobListFilter {
            status: Some(JobStatus::Running),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].id, b);

    let summary = store.summary_all().unwrap();
    let list_text = format_job_list(&completed, Some(&summary), Some(JobStatus::Completed));
    assert!(list_text.contains("completed"));
    assert!(list_text.contains("a.rs"));

    let detail = format_job_detail(&completed[0]);
    assert!(detail.contains("understood"));
    assert!(detail.contains(a.as_str()));
}

#[test]
fn job_status_parses_aliases() {
    assert_eq!("pending".parse::<JobStatus>().unwrap(), JobStatus::Pending);
    assert_eq!("done".parse::<JobStatus>().unwrap(), JobStatus::Completed);
    assert_eq!("error".parse::<JobStatus>().unwrap(), JobStatus::Failed);
    assert!("nope".parse::<JobStatus>().is_err());
}

#[test]
fn purge_older_than_removes_stale_jobs() {
    use chrono::{Duration, Utc};

    let store = Arc::new(MemoryJobStore::new());
    let mut aged = Job::new("s", "old", serde_json::json!({"n": 1}));
    aged.updated_at = Utc::now() - Duration::days(10);
    aged.created_at = aged.updated_at;
    let aged_id = store.enqueue(aged).unwrap();

    let recent = Job::new("s", "new", serde_json::json!({"n": 2}));
    let recent_id = store.enqueue(recent).unwrap();

    let cutoff = Utc::now() - Duration::days(5);
    let n = store.purge_older_than(cutoff, None).unwrap();
    assert_eq!(n, 1);
    assert!(store.get(&aged_id).unwrap().is_none());
    assert!(store.get(&recent_id).unwrap().is_some());
    assert_eq!(store.summary_all().unwrap().total(), 1);
}

#[test]
fn redb_purge_older_than() {
    use chrono::{Duration, Utc};

    let store = RedbJobStore::in_memory().unwrap();
    let mut aged = Job::new("s", "old", serde_json::json!({}));
    aged.updated_at = Utc::now() - Duration::hours(48);
    aged.created_at = aged.updated_at;
    store.enqueue(aged).unwrap();
    store
        .enqueue(Job::new("s", "new", serde_json::json!({})))
        .unwrap();

    let n = store
        .purge_older_than(Utc::now() - Duration::hours(24), None)
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(store.summary_all().unwrap().total(), 1);
}

#[test]
fn purge_and_list_by_scan_id() {
    use chrono::{Duration, Utc};

    let store = Arc::new(MemoryJobStore::new());
    store
        .enqueue(Job::new("scan:a", "k", serde_json::json!({"n": 1})))
        .unwrap();
    store
        .enqueue(Job::new("scan:a", "k", serde_json::json!({"n": 2})))
        .unwrap();
    store
        .enqueue(Job::new("scan:b", "k", serde_json::json!({"n": 3})))
        .unwrap();

    let listed = store
        .list(&JobListFilter {
            scan_id: Some("scan:a".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(listed.len(), 2);

    let n = store.purge_scan("scan:a").unwrap();
    assert_eq!(n, 2);
    assert_eq!(store.summary("scan:b").unwrap().total(), 1);
    assert_eq!(store.summary_all().unwrap().total(), 1);

    let mut aged = Job::new("scan:b", "old", serde_json::json!({}));
    aged.updated_at = Utc::now() - Duration::days(10);
    aged.created_at = aged.updated_at;
    store.enqueue(aged).unwrap();
    let n = store
        .purge_older_than(Utc::now() - Duration::days(5), Some("scan:b"))
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(store.summary("scan:b").unwrap().total(), 1);
}

#[test]
fn pause_stop_and_complete_noop() {
    use gnosis::{pause_scan, stop_jobs};

    let store = Arc::new(MemoryJobStore::new());
    let a = store
        .enqueue(Job::new("scan:p", "k", serde_json::json!({})))
        .unwrap();
    let b = store
        .enqueue(Job::new("scan:p", "k", serde_json::json!({})))
        .unwrap();

    let paused = pause_scan(store.as_ref(), "p").unwrap();
    assert_eq!(paused.len(), 2);
    assert_eq!(store.summary("scan:p").unwrap().paused, 2);
    assert!(store.claim_next("scan:p", "w").unwrap().is_none());

    store.unpause(&a).unwrap();
    let claimed = store.claim_next("scan:p", "w").unwrap().unwrap();
    assert_eq!(claimed.id, a);

    // Stop while running — worker complete should not overwrite.
    store.stop(&a).unwrap();
    store
        .complete(&a, serde_json::json!({"should": "ignore"}))
        .unwrap();
    let job = store.get(&a).unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Stopped);
    assert!(job.result.is_none());

    let stopped = stop_jobs(store.as_ref(), &[b.to_string()]).unwrap();
    assert_eq!(stopped.len(), 1);
    assert_eq!(store.get(&b).unwrap().unwrap().status, JobStatus::Stopped);
}

#[test]
fn prepare_rerun_requeues_comma_list() {
    use gnosis::{parse_job_id_list, prepare_rerun};

    let store = Arc::new(MemoryJobStore::new()) as Arc<dyn JobStore>;
    let a = store
        .enqueue(Job::new(
            "s",
            KIND_ANALYZE_OBJECT,
            serde_json::json!({"path": "/tmp/a"}),
        ))
        .unwrap();
    let _ = store.claim_next("s", "w").unwrap().unwrap();
    store.complete(&a, serde_json::json!({"ok": true})).unwrap();
    let short = a.as_str().rsplit(':').next().unwrap()[..8].to_string();
    let queries = parse_job_id_list(&short).unwrap();
    let report = prepare_rerun(Arc::clone(&store), &queries).unwrap();
    assert_eq!(report.requeued.len(), 1);
    let job = store.get(&a).unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Pending);
    assert!(job.scan_id.starts_with("rerun:"));
    assert!(job.result.is_none());
}

#[test]
fn redb_pause_unpause_stop_and_complete_noop() {
    let store = RedbJobStore::in_memory().unwrap();
    let scan = "scan:redb-ctrl";
    let a = store
        .enqueue(Job::new(scan, "k", serde_json::json!({"n": 1})))
        .unwrap();
    let b = store
        .enqueue(Job::new(scan, "k", serde_json::json!({"n": 2})))
        .unwrap();

    let paused = store.pause(&a).unwrap();
    assert_eq!(paused.status, JobStatus::Paused);
    assert!(store.claim_next(scan, "w").unwrap().is_some()); // claims b
    assert!(store.claim_next(scan, "w").unwrap().is_none());

    let unpaused = store.unpause(&a).unwrap();
    assert_eq!(unpaused.status, JobStatus::Pending);
    let claimed = store.claim_next(scan, "w").unwrap().unwrap();
    assert_eq!(claimed.id, a);

    store.stop(&a).unwrap();
    store
        .complete(&a, serde_json::json!({"ignore": true}))
        .unwrap();
    store.fail(&a, "ignore".into()).unwrap();
    let job = store.get(&a).unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Stopped);
    assert!(job.result.is_none());
    assert_eq!(job.error.as_deref(), Some("stopped by user"));

    // Terminal jobs cannot be paused/unpaused/stopped again.
    assert!(store.pause(&a).is_err());
    assert!(store.unpause(&a).is_err());
    assert!(store.stop(&a).is_err());

    // Pause a still-pending job via stop on b if it was claimed earlier — requeue first.
    let c = store
        .enqueue(Job::new(scan, "k", serde_json::json!({"n": 3})))
        .unwrap();
    store.pause(&c).unwrap();
    store.stop(&c).unwrap();
    assert_eq!(store.get(&c).unwrap().unwrap().status, JobStatus::Stopped);
    let _ = b; // keep enqueue of b intentional for FIFO coverage
}

#[test]
fn redb_retry_backoff_and_fail() {
    use chrono::{Duration as ChronoDuration, Utc};

    let store = RedbJobStore::in_memory().unwrap();
    let scan = "scan:redb-retry";
    let id = store
        .enqueue(Job::new(scan, "k", serde_json::json!({})))
        .unwrap();
    let _ = store.claim_next(scan, "w").unwrap().unwrap();

    store
        .schedule_retry(&id, "boom".into(), Utc::now() + ChronoDuration::seconds(60))
        .unwrap();
    let job = store.get(&id).unwrap().unwrap();
    assert_eq!(job.status, JobStatus::Pending);
    assert!(job.available_at.is_some());
    assert!(store.claim_next(scan, "w").unwrap().is_none());

    // Not running → schedule_retry is a no-op.
    store
        .schedule_retry(&id, "ignored".into(), Utc::now())
        .unwrap();
    assert_eq!(
        store.get(&id).unwrap().unwrap().error.as_deref(),
        Some("boom")
    );

    // Make claimable, claim, then fail terminally.
    store.requeue(&id, None).unwrap();
    let _ = store.claim_next(scan, "w").unwrap().unwrap();
    store.fail(&id, "gave up".into()).unwrap();
    let failed = store.get(&id).unwrap().unwrap();
    assert_eq!(failed.status, JobStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("gave up"));
    store
        .complete(&id, serde_json::json!({"no": true}))
        .unwrap();
    assert_eq!(store.get(&id).unwrap().unwrap().status, JobStatus::Failed);
}

#[test]
fn redb_reclaim_stale_running_jobs() {
    let store = RedbJobStore::in_memory().unwrap();
    let scan = "scan:redb-reclaim";
    let id = store
        .enqueue(Job::new(scan, "x", serde_json::json!({})))
        .unwrap();
    let _ = store.claim_next(scan, "w").unwrap().unwrap();
    assert_eq!(store.summary(scan).unwrap().running, 1);
    let n = store.reclaim_stale(scan).unwrap();
    assert_eq!(n, 1);
    assert_eq!(store.summary(scan).unwrap().pending, 1);
    let again = store.claim_next(scan, "w2").unwrap().unwrap();
    assert_eq!(again.id, id);
}

#[test]
fn job_store_count_matches_summary() {
    let store = MemoryJobStore::new();
    let scan = "scan:counts";

    let pending = store
        .enqueue(Job::new(scan, "k", serde_json::json!({"role": "pending"})))
        .unwrap();
    let running = store
        .enqueue(Job::new(scan, "k", serde_json::json!({"role": "running"})))
        .unwrap();
    let paused = store
        .enqueue(Job::new(scan, "k", serde_json::json!({"role": "paused"})))
        .unwrap();
    let completed = store
        .enqueue(Job::new(
            scan,
            "k",
            serde_json::json!({"role": "completed"}),
        ))
        .unwrap();
    let failed = store
        .enqueue(Job::new(scan, "k", serde_json::json!({"role": "failed"})))
        .unwrap();
    let stopped = store
        .enqueue(Job::new(scan, "k", serde_json::json!({"role": "stopped"})))
        .unwrap();

    // Drive each id into the intended status (FIFO claim order matches enqueue order).
    store.pause(&paused).unwrap();

    let claimed_pending = store.claim_next(scan, "w").unwrap().unwrap();
    assert_eq!(claimed_pending.id, pending);
    // Requeue pending back so it stays pending; claim running instead.
    store.requeue(&pending, None).unwrap();

    let claimed_running = store.claim_next(scan, "w").unwrap().unwrap();
    assert_eq!(claimed_running.id, running);
    // leave running as-is

    let claimed_completed = store.claim_next(scan, "w").unwrap().unwrap();
    assert_eq!(claimed_completed.id, completed);
    store.complete(&completed, serde_json::json!({})).unwrap();

    let claimed_failed = store.claim_next(scan, "w").unwrap().unwrap();
    assert_eq!(claimed_failed.id, failed);
    store.fail(&failed, "x".into()).unwrap();

    let claimed_stopped = store.claim_next(scan, "w").unwrap().unwrap();
    assert_eq!(claimed_stopped.id, stopped);
    store.stop(&stopped).unwrap();

    // pending was requeued and never claimed again → still pending
    assert_eq!(
        store.get(&pending).unwrap().unwrap().status,
        JobStatus::Pending
    );
    assert_eq!(
        store.get(&running).unwrap().unwrap().status,
        JobStatus::Running
    );

    let summary = store.summary(scan).unwrap();
    assert_eq!(summary.pending, 1);
    assert_eq!(summary.running, 1);
    assert_eq!(summary.paused, 1);
    assert_eq!(summary.completed, 1);
    assert_eq!(summary.failed, 1);
    assert_eq!(summary.stopped, 1);

    assert_eq!(store.count(scan, JobStatus::Pending).unwrap(), 1);
    assert_eq!(store.count(scan, JobStatus::Running).unwrap(), 1);
    assert_eq!(store.count(scan, JobStatus::Paused).unwrap(), 1);
    assert_eq!(store.count(scan, JobStatus::Completed).unwrap(), 1);
    assert_eq!(store.count(scan, JobStatus::Failed).unwrap(), 1);
    assert_eq!(store.count(scan, JobStatus::Stopped).unwrap(), 1);
}

#[test]
fn redb_list_scan_ids_and_purge_scan() {
    let store = RedbJobStore::in_memory().unwrap();
    store
        .enqueue(Job::new("scan:old", "k", serde_json::json!({})))
        .unwrap();
    store
        .enqueue(Job::new("scan:new", "k", serde_json::json!({})))
        .unwrap();
    let ids = store.list_scan_ids().unwrap();
    assert!(ids.contains(&"scan:old".to_string()));
    assert!(ids.contains(&"scan:new".to_string()));
    assert_eq!(store.purge_scan("scan:old").unwrap(), 1);
    assert_eq!(store.summary("scan:old").unwrap().total(), 0);
    assert_eq!(store.summary("scan:new").unwrap().total(), 1);
}
