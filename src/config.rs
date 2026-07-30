use crate::connectors::s3::S3Location;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Where objects are discovered from.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ScanSource {
    /// Local directory (and optional Git enrichment).
    Filesystem { root: PathBuf },
    /// S3 bucket treated as a root folder; object keys are paths.
    S3 {
        location: S3Location,
        /// Optional AWS region override (otherwise default chain / env).
        region: Option<String>,
    },
}

impl ScanSource {
    pub fn connector_name(&self) -> &'static str {
        match self {
            Self::Filesystem { .. } => "filesystem",
            Self::S3 { .. } => "s3",
        }
    }

    pub fn display_root(&self) -> PathBuf {
        match self {
            Self::Filesystem { root } => root.clone(),
            Self::S3 { location, .. } => location.display_root(),
        }
    }

    pub fn uses_git(&self) -> bool {
        matches!(self, Self::Filesystem { .. })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanConfig {
    pub source: ScanSource,
    /// Display / store root (`filesystem` path or `s3://bucket[/prefix]`).
    pub root: PathBuf,
    pub max_object_size: u64,
    pub excluded_paths: Vec<String>,
    pub concurrency: usize,
    pub event_history_len: usize,
    pub output_path: PathBuf,
    pub skip_output_dir_name: String,
    pub queue_capacity: usize,
}

impl Default for ScanConfig {
    fn default() -> Self {
        let root = PathBuf::from(".");
        Self {
            source: ScanSource::Filesystem { root: root.clone() },
            root,
            max_object_size: 2 * 1024 * 1024,
            excluded_paths: vec![
                "target".into(),
                "node_modules".into(),
                ".git".into(),
                "knowledge.okf".into(),
            ],
            concurrency: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .min(8),
            event_history_len: 200,
            output_path: PathBuf::from("knowledge.okf"),
            skip_output_dir_name: "knowledge.okf".into(),
            queue_capacity: 1024,
        }
    }
}

impl ScanConfig {
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            source: ScanSource::Filesystem { root: root.clone() },
            root,
            ..Self::default()
        }
    }

    pub fn with_source(source: ScanSource) -> Self {
        let root = source.display_root();
        Self {
            source,
            root,
            ..Self::default()
        }
    }

    pub fn connector_name(&self) -> &'static str {
        self.source.connector_name()
    }
}

#[derive(Debug, Default)]
pub struct ScanMetrics {
    pub objects_discovered: AtomicU64,
    pub bytes_considered: AtomicU64,
    pub understood: AtomicU64,
    pub partial: AtomicU64,
    pub unknown: AtomicU64,
    pub failed: AtomicU64,
    pub entities: AtomicU64,
    pub relationships: AtomicU64,
    started: std::sync::Mutex<Option<Instant>>,
}

impl ScanMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&self) {
        *self.started.lock().unwrap() = Some(Instant::now());
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started
            .lock()
            .unwrap()
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }

    pub fn snapshot(&self, queue_depth: usize) -> crate::events::MetricsSnapshot {
        crate::events::MetricsSnapshot {
            objects_discovered: self.objects_discovered.load(Ordering::Relaxed),
            bytes_considered: self.bytes_considered.load(Ordering::Relaxed),
            understood: self.understood.load(Ordering::Relaxed),
            partial: self.partial.load(Ordering::Relaxed),
            unknown: self.unknown.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            entities: self.entities.load(Ordering::Relaxed),
            relationships: self.relationships.load(Ordering::Relaxed),
            queue_depth,
            elapsed_ms: self.elapsed_ms(),
        }
    }

    pub fn record_status(&self, status: crate::status::UnderstandingStatus) {
        use crate::status::UnderstandingStatus::*;
        match status {
            Understood => self.understood.fetch_add(1, Ordering::Relaxed),
            PartiallyUnderstood => self.partial.fetch_add(1, Ordering::Relaxed),
            Unknown => self.unknown.fetch_add(1, Ordering::Relaxed),
            Failed => self.failed.fetch_add(1, Ordering::Relaxed),
        };
    }
}
