use crate::config::{ScanConfig, ScanMetrics, ScanSource};
use crate::connectors::filesystem::{
    collect_neighbors, fingerprint_bytes, permissions_string, read_object_bytes,
    FilesystemConnector,
};
use crate::connectors::git::GitContext;
use crate::connectors::s3::{AwsS3Backend, S3Connector};
use crate::connectors::types::{ObjectDescriptor, ProtoData};
use crate::content::BytesContentReader;
use crate::error::{GnosisError, Result};
use crate::events::PipelineEvent;
use crate::provider::ProviderRegistry;
use crate::status::UnderstandingStatus;
use crate::store::{KnowledgeStore, StoredObject};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub struct Pipeline {
    config: ScanConfig,
    providers: ProviderRegistry,
    store: Arc<Mutex<KnowledgeStore>>,
    metrics: Arc<ScanMetrics>,
    cancel: Arc<AtomicBool>,
}

pub struct PipelineHandle {
    pub events: Option<Receiver<PipelineEvent>>,
    pub store: Arc<Mutex<KnowledgeStore>>,
    pub metrics: Arc<ScanMetrics>,
    pub cancel: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<Result<()>>>,
}

impl PipelineHandle {
    pub fn take_events(&mut self) -> Receiver<PipelineEvent> {
        self.events
            .take()
            .expect("events already taken from PipelineHandle")
    }

    pub fn wait(&mut self) -> Result<()> {
        if let Some(handle) = self.join.take() {
            handle
                .join()
                .map_err(|_| GnosisError::Pipeline("worker panicked".into()))?
        } else {
            Ok(())
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// How workers fetch object bytes / neighbors for a scan.
#[derive(Clone)]
enum ObjectAccess {
    Filesystem,
    S3(Arc<S3Connector<AwsS3Backend>>),
}

impl ObjectAccess {
    fn connector_name(&self) -> &'static str {
        match self {
            Self::Filesystem => "filesystem",
            Self::S3(_) => "s3",
        }
    }

    fn read_bytes(&self, object: &ObjectDescriptor, max_size: u64) -> Result<Vec<u8>> {
        match self {
            Self::Filesystem => read_object_bytes(&object.path, max_size),
            Self::S3(c) => c.read_object_bytes(object, max_size),
        }
    }

    fn neighbors(&self, object: &ObjectDescriptor, limit: usize) -> Vec<String> {
        match self {
            Self::Filesystem => collect_neighbors(&object.path, limit),
            Self::S3(c) => c.collect_neighbors(object, limit),
        }
    }

    fn permissions(&self, object: &ObjectDescriptor) -> Option<String> {
        match self {
            Self::Filesystem => permissions_string(&object.path),
            Self::S3(_) => None,
        }
    }
}

impl Pipeline {
    pub fn new(config: ScanConfig, providers: ProviderRegistry) -> Self {
        let mut store = KnowledgeStore::new();
        store.set_root(config.root.clone());
        store.set_connector(config.connector_name().to_string());
        store.set_enabled_providers(
            providers
                .coverage_summary()
                .into_iter()
                .map(|(id, _)| id)
                .collect(),
        );
        Self {
            config,
            providers,
            store: Arc::new(Mutex::new(store)),
            metrics: Arc::new(ScanMetrics::new()),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn store(&self) -> Arc<Mutex<KnowledgeStore>> {
        Arc::clone(&self.store)
    }

    pub fn metrics(&self) -> Arc<ScanMetrics> {
        Arc::clone(&self.metrics)
    }

    /// Run the scan on a background thread; returns an event receiver.
    pub fn spawn(self) -> PipelineHandle {
        let (tx, rx) = bounded(self.config.event_history_len.max(64));
        let store = Arc::clone(&self.store);
        let metrics = Arc::clone(&self.metrics);
        let cancel = Arc::clone(&self.cancel);
        let join = thread::spawn(move || self.run(tx));
        PipelineHandle {
            events: Some(rx),
            store,
            metrics,
            cancel,
            join: Some(join),
        }
    }

    /// Run synchronously, forwarding events to `tx`.
    pub fn run(self, events: Sender<PipelineEvent>) -> Result<()> {
        self.metrics.start();
        let root = self.config.root.clone();
        let _ = events.send(PipelineEvent::ScanStarted { root: root.clone() });

        let git = if self.config.source.uses_git() {
            let git = GitContext::detect(&root);
            {
                let mut store = self.store.lock().unwrap();
                store.set_git_branch(git.branch.clone());
            }
            git
        } else {
            GitContext::default()
        };

        let (objects, access) = match &self.config.source {
            ScanSource::Filesystem { .. } => {
                let connector = FilesystemConnector::new(self.config.clone());
                let objects = match connector.discover(&events, &self.cancel) {
                    Ok(o) => o,
                    Err(GnosisError::Cancelled) => return Err(GnosisError::Cancelled),
                    Err(e) => {
                        let _ = events.send(PipelineEvent::Failure {
                            id: None,
                            message: e.to_string(),
                        });
                        return Err(e);
                    }
                };
                (objects, ObjectAccess::Filesystem)
            }
            ScanSource::S3 { location, region } => {
                let backend = Arc::new(AwsS3Backend::new(region.as_deref())?);
                let connector = Arc::new(S3Connector::new(
                    location.clone(),
                    self.config.clone(),
                    backend,
                ));
                let objects = match connector.discover(&events, &self.cancel) {
                    Ok(o) => o,
                    Err(GnosisError::Cancelled) => return Err(GnosisError::Cancelled),
                    Err(e) => {
                        let _ = events.send(PipelineEvent::Failure {
                            id: None,
                            message: e.to_string(),
                        });
                        return Err(e);
                    }
                };
                (objects, ObjectAccess::S3(connector))
            }
        };

        self.metrics
            .objects_discovered
            .store(objects.len() as u64, Ordering::Relaxed);

        let (work_tx, work_rx) = bounded::<ObjectDescriptor>(self.config.queue_capacity);
        let workers = self.config.concurrency.max(1);

        let providers = Arc::new(self.providers);
        let mut thread_handles = Vec::new();
        for _ in 0..workers {
            let work_rx = work_rx.clone();
            let events = events.clone();
            let store = Arc::clone(&self.store);
            let metrics = Arc::clone(&self.metrics);
            let cancel = Arc::clone(&self.cancel);
            let config = self.config.clone();
            let git = git.clone();
            let providers = Arc::clone(&providers);
            let access = access.clone();
            let use_git = self.config.source.uses_git();
            thread_handles.push(thread::spawn(move || {
                while let Ok(object) = work_rx.recv() {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Err(e) = process_object(
                        &object, &providers, &config, &git, use_git, &access, &store, &metrics,
                        &events,
                    ) {
                        let _ = events.send(PipelineEvent::Failure {
                            id: Some(object.id.clone()),
                            message: e.to_string(),
                        });
                        metrics.record_status(UnderstandingStatus::Failed);
                    }
                }
            }));
        }

        for object in objects {
            if self.cancel.load(Ordering::Relaxed) {
                break;
            }
            let depth = work_tx.len();
            let _ = events.send(PipelineEvent::ObjectQueued {
                id: object.id.clone(),
                queue_depth: depth,
            });
            if work_tx.send(object).is_err() {
                break;
            }
        }
        drop(work_tx);

        for h in thread_handles {
            let _ = h.join();
        }

        let elapsed = self.metrics.elapsed_ms();
        let discovered = self.metrics.objects_discovered.load(Ordering::Relaxed);
        let _ = events.send(PipelineEvent::MetricsSnapshot {
            metrics: self.metrics.snapshot(0),
        });
        let _ = events.send(PipelineEvent::ScanCompleted {
            objects: discovered,
            elapsed_ms: elapsed,
        });

        // Keep sender alive briefly so receivers drain.
        thread::sleep(Duration::from_millis(10));
        Ok(())
    }
}

fn process_object(
    object: &ObjectDescriptor,
    providers: &ProviderRegistry,
    config: &ScanConfig,
    git: &GitContext,
    use_git: bool,
    access: &ObjectAccess,
    store: &Mutex<KnowledgeStore>,
    metrics: &ScanMetrics,
    events: &Sender<PipelineEvent>,
) -> Result<()> {
    metrics
        .bytes_considered
        .fetch_add(object.size, Ordering::Relaxed);

    let bytes = access.read_bytes(object, config.max_object_size)?;
    let fingerprint = fingerprint_bytes(&bytes);

    let proto = ProtoData {
        connector: access.connector_name().into(),
        path: object.path.clone(),
        relative_path: object.relative_path.clone(),
        filename: object
            .relative_path
            .file_name()
            .or_else(|| object.path.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        extension: object.extension.clone(),
        parent_path: object.relative_path.parent().map(|p| p.to_path_buf()),
        neighbor_names: access.neighbors(object, 16),
        media_type: object.media_type.clone(),
        size: object.size,
        modified: object.modified,
        permissions: access.permissions(object),
        fingerprint: Some(fingerprint),
        git: if use_git {
            git.enrich(&object.path)
        } else {
            None
        },
    };

    let (provider, support) = match providers.select(object, &proto) {
        Some(pair) => pair,
        None => {
            let reason = "no provider available".to_string();
            let stored = StoredObject {
                descriptor: object.clone(),
                proto,
                status: UnderstandingStatus::Unknown,
                provider: None,
                classification_reason: Some(reason.clone()),
                entity_ids: Vec::new(),
                diagnostics: Vec::new(),
            };
            store.lock().unwrap().upsert_object(stored);
            metrics.record_status(UnderstandingStatus::Unknown);
            let _ = events.send(PipelineEvent::ObjectClassified {
                id: object.id.clone(),
                status: UnderstandingStatus::Unknown,
                reason: Some(reason),
            });
            return Ok(());
        }
    };

    let _ = events.send(PipelineEvent::ProviderSelected {
        id: object.id.clone(),
        provider: provider.id(),
        support: support.as_str().into(),
    });
    let _ = events.send(PipelineEvent::AnalysisStarted {
        id: object.id.clone(),
        provider: provider.id(),
    });

    let mut reader = BytesContentReader::new(bytes);
    let result = provider.analyze(object, &proto, &mut reader)?;

    let mut entity_ids = Vec::new();
    {
        let mut st = store.lock().unwrap();
        for entity in &result.record.entities {
            entity_ids.push(entity.id.clone());
            let _ = events.send(PipelineEvent::EntityCreated {
                entity: entity.clone(),
            });
            st.add_entity(entity.clone());
            metrics.entities.fetch_add(1, Ordering::Relaxed);
        }
        for rel in &result.record.relationships {
            let _ = events.send(PipelineEvent::RelationshipCreated {
                kind: rel.kind.clone(),
                from: rel.from.to_string(),
                to: rel.to.to_string(),
            });
            st.add_relationship(rel.clone());
            metrics.relationships.fetch_add(1, Ordering::Relaxed);
        }

        let diagnostics: Vec<String> = result
            .record
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();

        st.upsert_object(StoredObject {
            descriptor: object.clone(),
            proto,
            status: result.status,
            provider: Some(provider.id()),
            classification_reason: result.classification_reason.clone(),
            entity_ids,
            diagnostics,
        });
    }

    metrics.record_status(result.status);
    let _ = events.send(PipelineEvent::AnalysisCompleted {
        id: object.id.clone(),
        provider: provider.id(),
        status: result.status,
        entities: result.record.entities.len(),
        relationships: result.record.relationships.len(),
    });
    let _ = events.send(PipelineEvent::ObjectClassified {
        id: object.id.clone(),
        status: result.status,
        reason: result.classification_reason,
    });

    Ok(())
}
