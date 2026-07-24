use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanConfig {
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
        Self {
            root: PathBuf::from("."),
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
        Self {
            root: root.into(),
            ..Self::default()
        }
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
