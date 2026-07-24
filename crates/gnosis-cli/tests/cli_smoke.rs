use assert_cmd::cargo::cargo_bin;
use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;
use tempfile::TempDir;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mixed-repo")
}

#[test]
fn cli_about_and_help() {
    Command::new(cargo_bin!("gnosis"))
        .arg("about")
        .assert()
        .success()
        .stdout(predicate::str::contains("Enterprise Knowledge Compiler"));

    Command::new(cargo_bin!("gnosis"))
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("scan"));
}

#[test]
fn cli_scan_headless_summary() {
    Command::new(cargo_bin!("gnosis"))
        .args(["scan", fixture().to_str().unwrap(), "--no-tui", "--quiet"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Gnosis summary"))
        .stdout(predicate::str::contains("unknown / partial"));
}

#[test]
fn cli_scan_headless_export() {
    let dir = TempDir::new().unwrap();
    let out = dir.path().join("out.okf");
    Command::new(cargo_bin!("gnosis"))
        .args([
            "scan",
            fixture().to_str().unwrap(),
            "--no-tui",
            "--quiet",
            "--export",
            "--output",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("export complete"));
    assert!(out.join("index.md").exists());
    assert!(out.join("sidecar.json").exists());
}

#[test]
fn cli_scan_missing_path_fails() {
    Command::new(cargo_bin!("gnosis"))
        .args(["scan", "/no/such/gnosis/path", "--no-tui"])
        .assert()
        .failure();
}
