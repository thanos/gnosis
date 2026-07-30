use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Stable identifier for a persisted job.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(String);

impl JobId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn generate() -> Self {
        Self(format!("job:{}", uuid::Uuid::new_v4()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Lifecycle of a job on the queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    /// Suspended — not claimable until unpaused.
    Paused,
    Completed,
    Failed,
    /// Cancelled by the user — terminal (use rerun/requeue to retry).
    Stopped,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Stopped)
    }

    pub fn all() -> [Self; 6] {
        [
            Self::Pending,
            Self::Running,
            Self::Paused,
            Self::Completed,
            Self::Failed,
            Self::Stopped,
        ]
    }
}

impl fmt::Display for JobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for JobStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pending" | "pend" | "queued" | "queue" => Ok(Self::Pending),
            "running" | "run" | "active" => Ok(Self::Running),
            "paused" | "pause" => Ok(Self::Paused),
            "completed" | "complete" | "done" | "ok" | "success" => Ok(Self::Completed),
            "failed" | "fail" | "error" | "err" => Ok(Self::Failed),
            "stopped" | "stop" | "cancelled" | "canceled" | "cancel" => Ok(Self::Stopped),
            other => Err(format!(
                "unknown job status '{other}' (pending|running|paused|completed|failed|stopped)"
            )),
        }
    }
}

/// Persisted job: function kind, arguments, and result/error.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    /// Logical scan / batch this job belongs to.
    pub scan_id: String,
    /// Function / handler name (e.g. `analyze_object`).
    pub kind: String,
    /// Serialized arguments for the job function.
    pub args: serde_json::Value,
    pub status: JobStatus,
    /// Serialized successful result (when `Completed`).
    pub result: Option<serde_json::Value>,
    /// Error message (when `Failed` or `Stopped`).
    pub error: Option<String>,
    pub attempts: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    /// When set, the job is not claimable until this time (retry backoff).
    #[serde(default)]
    pub available_at: Option<DateTime<Utc>>,
    /// Worker that last claimed the job.
    pub worker_id: Option<String>,
}

impl Job {
    pub fn new(
        scan_id: impl Into<String>,
        kind: impl Into<String>,
        args: serde_json::Value,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: JobId::generate(),
            scan_id: scan_id.into(),
            kind: kind.into(),
            args,
            status: JobStatus::Pending,
            result: None,
            error: None,
            attempts: 0,
            created_at: now,
            updated_at: now,
            started_at: None,
            finished_at: None,
            available_at: None,
            worker_id: None,
        }
    }

    /// Whether a pending job may be claimed right now.
    pub fn is_claimable_at(&self, now: DateTime<Utc>) -> bool {
        self.status == JobStatus::Pending && self.available_at.map(|t| t <= now).unwrap_or(true)
    }
}

/// Filters for listing persisted jobs.
#[derive(Clone, Debug, Default)]
pub struct JobListFilter {
    pub scan_id: Option<String>,
    pub status: Option<JobStatus>,
    /// Max rows to return (after filtering / sort). `None` = no limit.
    pub limit: Option<usize>,
}

/// Aggregate counts for jobs (optionally scoped to a scan).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobSummary {
    pub pending: u64,
    pub running: u64,
    pub paused: u64,
    pub completed: u64,
    pub failed: u64,
    pub stopped: u64,
}

impl JobSummary {
    /// Jobs still in flight for worker drain (pending + running).
    pub fn active(&self) -> u64 {
        self.pending + self.running
    }

    pub fn total(&self) -> u64 {
        self.pending + self.running + self.paused + self.completed + self.failed + self.stopped
    }

    pub fn record(&mut self, status: JobStatus) {
        match status {
            JobStatus::Pending => self.pending += 1,
            JobStatus::Running => self.running += 1,
            JobStatus::Paused => self.paused += 1,
            JobStatus::Completed => self.completed += 1,
            JobStatus::Failed => self.failed += 1,
            JobStatus::Stopped => self.stopped += 1,
        }
    }
}
