use gnosis_core::{FilesystemConnector, GitContext, PipelineEvent, ScanConfig};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn filesystem_connector_respects_gitignore_and_excludes() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("target/debug")).unwrap();
    fs::create_dir_all(root.join("ignored")).unwrap();
    fs::write(root.join("src/main.rs"), b"fn main() {}").unwrap();
    fs::write(root.join("target/debug/x.o"), b"obj").unwrap();
    fs::write(root.join("ignored/secret.bin"), b"secret").unwrap();
    fs::write(root.join(".gitignore"), b"ignored/\n").unwrap();
    fs::create_dir_all(root.join("knowledge.okf")).unwrap();
    fs::write(root.join("knowledge.okf/index.md"), b"# x").unwrap();

    let config = ScanConfig::with_root(root);
    let connector = FilesystemConnector::new(config);
    let (tx, rx) = crossbeam_channel::unbounded::<PipelineEvent>();
    let cancel = Arc::new(AtomicBool::new(false));
    let objects = connector.discover(&tx, &cancel).unwrap();
    drop(tx);
    let _ = rx.try_iter().count();

    let paths: Vec<_> = objects
        .iter()
        .map(|o| o.relative_path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(paths.iter().any(|p| p.ends_with("main.rs")));
    assert!(!paths.iter().any(|p| p.contains("secret.bin")));
    assert!(!paths.iter().any(|p| p.contains("target/")));
    assert!(!paths.iter().any(|p| p.contains("knowledge.okf")));
}

#[test]
fn git_context_detects_or_falls_back() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().unwrap();
    let ctx = GitContext::detect(&root);
    if ctx.repository_root.is_some() {
        let enrich = ctx.enrich(&root.join("README.md"));
        assert!(enrich.is_some());
    }
}

#[test]
fn read_object_bytes_respects_max_size() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("big.bin");
    fs::write(&path, vec![7u8; 100]).unwrap();
    let bytes = gnosis_core::connectors::filesystem::read_object_bytes(&path, 10).unwrap();
    assert_eq!(bytes.len(), 10);
}
