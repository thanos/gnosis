use crate::config::{ScanConfig, ScanMetrics};
use crate::connectors::filesystem::{
    collect_neighbors, fingerprint_bytes, permissions_string, read_object_bytes,
    FilesystemConnector,
};
use crate::connectors::git::GitContext;
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

impl Pipeline {
    pub fn new(config: ScanConfig, providers: ProviderRegistry) -> Self {
        let mut store = KnowledgeStore::new();
        store.set_root(config.root.clone());
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

        let git = GitContext::detect(&root);
        {
            let mut store = self.store.lock().unwrap();
            store.set_git_branch(git.branch.clone());
        }

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

        self.metrics
            .objects_discovered
            .store(objects.len() as u64, Ordering::Relaxed);

        let (work_tx, work_rx) = bounded::<ObjectDescriptor>(self.config.queue_capacity);
        let workers = self.config.concurrency.max(1);

        let mut handles = Vec::new();
        for _ in 0..workers {
            let work_rx = work_rx.clone();
            let events = events.clone();
            let store = Arc::clone(&self.store);
            let metrics = Arc::clone(&self.metrics);
            let cancel = Arc::clone(&self.cancel);
            let config = self.config.clone();
            let git = git.clone();
            // ProviderRegistry is not Clone — share via Arc
            // We'll move providers into Arc before spawning
            handles.push((work_rx, events, store, metrics, cancel, config, git));
        }

        // Rebuild with Arc providers
        let providers = Arc::new(self.providers);
        let mut thread_handles = Vec::new();
        for (work_rx, events, store, metrics, cancel, config, git) in handles {
            let providers = Arc::clone(&providers);
            thread_handles.push(thread::spawn(move || {
                while let Ok(object) = work_rx.recv() {
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Err(e) = process_object(
                        &object, &providers, &config, &git, &store, &metrics, &events,
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
    store: &Mutex<KnowledgeStore>,
    metrics: &ScanMetrics,
    events: &Sender<PipelineEvent>,
) -> Result<()> {
    metrics
        .bytes_considered
        .fetch_add(object.size, Ordering::Relaxed);

    let bytes = read_object_bytes(&object.path, config.max_object_size)?;
    let fingerprint = fingerprint_bytes(&bytes);

    let proto = ProtoData {
        connector: "filesystem".into(),
        path: object.path.clone(),
        relative_path: object.relative_path.clone(),
        filename: object
            .path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        extension: object.extension.clone(),
        parent_path: object.path.parent().map(|p| p.to_path_buf()),
        neighbor_names: collect_neighbors(&object.path, 16),
        media_type: object.media_type.clone(),
        size: object.size,
        modified: object.modified,
        permissions: permissions_string(&object.path),
        fingerprint: Some(fingerprint),
        git: git.enrich(&object.path),
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
