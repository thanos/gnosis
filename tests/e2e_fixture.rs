use gnosis::default_registry;
use gnosis::OkfExporter;
use gnosis::{Exporter, MemoryJobStore, Pipeline, QueryEngine, ScanConfig, UnderstandingStatus};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/mixed-repo")
}

fn mtimes(root: &std::path::Path) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    for entry in walkdir_simple(root) {
        if let Ok(meta) = std::fs::metadata(&entry) {
            if meta.is_file() {
                if let Ok(m) = meta.modified() {
                    out.push((entry, m));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn walkdir_simple(root: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".git" || name == "knowledge.okf" {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn end_to_end_fixture_scan() {
    let root = fixture_root();
    if !root.exists() {
        eprintln!(
            "skipping e2e: fixture repo missing at {} (expected in git checkout)",
            root.display()
        );
        return;
    }
    let root = root.canonicalize().expect("fixture mixed-repo must exist");

    let before = mtimes(&root);

    let mut config = ScanConfig::with_root(root.clone());
    config.concurrency = 2;
    config.output_path = std::env::temp_dir().join(format!(
        "gnosis-e2e-{}.okf",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    ));

    let pipeline = Pipeline::with_job_store(
        config.clone(),
        default_registry(),
        Arc::new(MemoryJobStore::new()),
    );
    let mut handle = pipeline.spawn();
    let events = handle.take_events();

    // Drain events until scan completes.
    let mut completed = false;
    while let Ok(ev) = events.recv() {
        if matches!(ev, gnosis::PipelineEvent::ScanCompleted { .. }) {
            completed = true;
            break;
        }
    }
    assert!(completed, "scan should complete");
    handle.wait().expect("pipeline ok");

    let store = handle.store.lock().unwrap();
    let q = QueryEngine::new(&store);
    let inv = q.stats();

    assert!(
        inv.source_objects >= 8,
        "expected multiple source objects, got {}",
        inv.source_objects
    );
    assert!(
        inv.functions + inv.types + inv.modules > 0,
        "expected code entities"
    );
    assert!(
        inv.documents + inv.datasets > 0,
        "expected doc/dataset entities"
    );

    // Binary should be unknown or partial.
    let unknown = q.unknown();
    assert!(
        unknown.iter().any(|o| {
            o.descriptor
                .relative_path
                .to_string_lossy()
                .contains("blob.bin")
        }),
        "blob.bin should appear in unknown/partial inventory"
    );

    // Ignored path should not be present.
    assert!(
        !store.objects().any(|o| {
            o.descriptor
                .relative_path
                .to_string_lossy()
                .contains("secret.bin")
        }),
        "gitignore'd secret.bin must not be scanned"
    );

    // Queries
    let found = q.find("PricingEngine");
    assert!(
        !found.entities.is_empty() || !found.objects.is_empty(),
        "find PricingEngine should hit"
    );
    assert!(q.explain("Catalog").is_some() || !q.find("Catalog").entities.is_empty());
    let _ = q.graph("Catalog", 1);

    // Export
    let exporter = OkfExporter::new();
    exporter
        .export(&store, &config.output_path)
        .expect("export okf");
    assert!(config.output_path.join("index.md").exists());
    assert!(config.output_path.join("sidecar.json").exists());
    assert!(config.output_path.join("entities").is_dir());

    drop(store);

    let after = mtimes(&root);
    assert_eq!(
        before.len(),
        after.len(),
        "source file count must not change"
    );
    for ((p1, m1), (p2, m2)) in before.iter().zip(after.iter()) {
        assert_eq!(p1, p2);
        assert_eq!(m1, m2, "source file modified: {}", p1.display());
    }

    let _ = std::fs::remove_dir_all(&config.output_path);
    let _ = UnderstandingStatus::Understood;
}
