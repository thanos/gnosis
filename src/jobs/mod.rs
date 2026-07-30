//! Persistent asynchronous job processing.
//!
//! Jobs record a function kind, JSON arguments, and eventual result or error.
//! Persistence is abstracted by [`JobStore`]; [`RedbJobStore`] is the default.

mod age;
mod control;
mod kinds;
mod memory;
mod redb_store;
mod rerun;
mod retry;
mod store;
mod types;
mod view;
mod worker;

pub use age::parse_age;
pub use control::{pause_jobs, pause_scan, stop_jobs, stop_scan, unpause_jobs, unpause_scan};
pub use kinds::{AnalyzeObjectArgs, AnalyzeObjectResult, KIND_ANALYZE_OBJECT};
pub use memory::MemoryJobStore;
pub use redb_store::RedbJobStore;
pub use rerun::{
    new_rerun_scan_id, new_scan_id, parse_job_id_list, prepare_rerun, prepare_rerun_scan,
    requeue_jobs, requeue_scan, resolve_job_id, resolve_scan_id, RerunReport,
};
pub use retry::RetryPolicy;
pub use store::JobStore;
pub use types::{Job, JobId, JobListFilter, JobStatus, JobSummary};
pub use view::{
    format_job_detail, format_job_list, format_job_list_filtered, format_job_list_line,
};
pub use worker::{JobExecutor, JobWorkerPool};
