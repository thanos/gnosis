# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Collapsed the multi-crate workspace into a **single** `gnosis` package (library + `gnosis` binary). Internal modules remain (`providers`, `okf`, `tui`, …); crates.io publish is one crate only.

## [0.1.0] - 2026-07-24

First public release of **Gnosis** — Enterprise Knowledge Compiler.

### Added

- Cargo workspace with crates:
  - `gnosis` (CLI binary)
  - `gnosis-core` (domain types, filesystem connector, pipeline, store, query)
  - `gnosis-providers` (Tree-sitter C++/Rust/Elixir + Markdown/text/JSON/YAML/TOML/CSV + generic metadata)
  - `gnosis-okf` (OKF-style markdown+YAML export behind an `Exporter` trait)
  - `gnosis-tui` (Ratatui live scan UI)
- `gnosis scan <path>` with live TUI or `--no-tui` headless mode
- Recursive discovery respecting `.gitignore`, hard excludes (`target`, `node_modules`, `.git`, `knowledge.okf`), and no symlink following
- ProtoData collection and local Git enrichment via the `git` CLI when available
- Deterministic understanding status: understood / partial / unknown / failed
- Query commands: `summary`, `objects`, `unknown`, `providers`, `stats`, `find`, `explain`, `graph`, `export okf`
- OKF bundle export (`index.md`, `entities/`, `objects/`, `relationships/`, `sidecar.json`, `log.md`)
- Fixture repo (`fixtures/mixed-repo`) and end-to-end / CLI smoke / provider / TUI tests
- CI: fmt, clippy, multi-OS test, llvm-cov → Coveralls (≥80% lines), audit/`cargo deny`, release build, package checks
- Release workflow: multi-platform binaries + GitHub Release + crates.io publish on `v*` tags
- Dependabot + scheduled dependency workflow

### Notes

- PoC scope: local filesystem only; no LLM providers, vectors, remote connectors, or `gnosis.toml`
- OKF export is a hand-written markdown+YAML bundle (not coupled to a specific OKF library crate)
- MSRV: Rust **1.85**

[Unreleased]: https://github.com/thanos/gnosis/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/thanos/gnosis/releases/tag/v0.1.0
