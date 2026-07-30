# Gnosis

[![CI](https://github.com/thanos/gnosis/actions/workflows/ci.yml/badge.svg)](https://github.com/thanos/gnosis/actions/workflows/ci.yml)
[![Coverage Status](https://coveralls.io/repos/github/thanos/gnosis/badge.svg?branch=main)](https://coveralls.io/github/thanos/gnosis?branch=main)
[![crates.io](https://img.shields.io/crates/v/gnosis.svg)](https://crates.io/crates/gnosis)
[![docs.rs](https://docs.rs/gnosis/badge.svg)](https://docs.rs/gnosis)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)

**Gnosis compiles what you have into what you know.**

Gnosis is an enterprise knowledge compiler: a local-first Rust tool that discovers digital objects in a repository or S3 bucket, extracts structured knowledge with deterministic providers (Tree-sitter and lightweight document/data parsers), shows its work in a live TUI, and exports an [OKF](https://github.com/GoogleCloudPlatform/knowledge-catalog) v0.1-style bundle.

This is a proof of concept — not a search engine, vector database, or chat UI.

<img width="600" alt="Screenshot 2026-07-24 at 15 40 14" src="https://github.com/user-attachments/assets/1dc94cac-473b-4d1f-bc8e-9bcdc4e88d24" />


## Install

### From crates.io

```bash
cargo install gnosis
```

### From source

```bash
cargo install --git https://github.com/thanos/gnosis
# or from a checkout:
cargo install --path .
```

Prebuilt binaries for Linux, macOS, and Windows are attached to [GitHub Releases](https://github.com/thanos/gnosis/releases) on each `v*` tag (with `SHA256SUMS`).

## Quick start

```sh
cargo build --release
./target/release/gnosis scan ./fixtures/mixed-repo
```

Headless (CI / scripting):

```sh
cargo run -- scan ./fixtures/mixed-repo --no-tui --export
```

S3 (bucket = root, keys = paths; default AWS credentials):

```sh
gnosis scan s3://my-bucket --no-tui --export
gnosis scan s3://my-bucket/path/prefix --region us-east-1
```

## Commands

| Command | Description |
|---------|-------------|
| `gnosis scan <path>` | Live TUI scan (local directory) |
| `gnosis scan s3://bucket[/prefix]` | Scan an S3 bucket |
| `gnosis scan <target> --no-tui` | Headless scan + summary |
| `gnosis scan <target> --no-tui --export` | Also write `knowledge.okf/` |
| `gnosis about` | Product overview |

### `scan` options

| Flag | Description |
|------|-------------|
| `--no-tui` | Headless mode (prints summary) |
| `--quiet` | Suppress per-object event lines in headless mode |
| `--export` | Write OKF when the scan finishes (headless) |
| `--output <dir>` | OKF output directory (default: `knowledge.okf`) |
| `--max-size <bytes>` | Max bytes read per object (default: 2 MiB) |
| `--concurrency <n>` | Analysis worker count |
| `--region <name>` | AWS region for `s3://` scans (default credential chain otherwise) |

Local scans are recursive under `<path>` (via `ignore::WalkBuilder`). Descent skips `.gitignore` matches, `target` / `node_modules` / `.git` / `knowledge.okf`, and does not follow symlinks.

S3 scans list object keys under the bucket (or prefix), skip directory markers (`…/`), and skip keys whose path components match the same basename excludes (`target`, `node_modules`, `.git`, `knowledge.okf`).
### TUI commands (`:`)

`summary` · `objects [status]` · `unknown` · `providers` · `stats` · `find <text>` · `explain <name>` · `graph <name>` · `export okf [path]` · `quit`

Shortcuts: `s` summary · `u` unknown · `e` export · `q` quit

## What you get

1. A live (or printed) inventory of what was **understood**, **partial**, **unknown**, or **failed**
2. Extracted entities and relationships (functions, classes, modules, documents, CSV columns, …)
3. Optional **OKF bundle** (`knowledge.okf/` by default) — markdown + YAML you can browse, commit, or feed to other tools

## Good fit today

- Point at an unfamiliar local Git/repo tree and see structure emerge
- Scan an S3 bucket the same way (keys as file paths) from a laptop or CI
- Demo / PoC of “compile tree → structured knowledge”
- Export a portable knowledge directory for humans or agents

Not for yet: remote Git hosts, web crawl, PDFs/office, vectors/RAG, or chat (see [Limitations](#limitations)).

## Library usage

One crate provides both the CLI and the library. Depend on it as:

```toml
[dependencies]
gnosis = "0.1"
```

Public modules include pipeline/store/query APIs plus `providers`, `okf`, and `tui` for embedding.

Headless scan + export sketch:

```rust
use gnosis::{
    default_registry, Exporter, OkfExporter, Pipeline, PipelineEvent, QueryEngine, Result,
    ScanConfig,
};

fn main() -> Result<()> {
    let config = ScanConfig::with_root("./my-repo");
    let pipeline = Pipeline::new(config.clone(), default_registry());
    let mut handle = pipeline.spawn();
    let events = handle.take_events();

    while let Ok(ev) = events.recv() {
        if matches!(ev, PipelineEvent::ScanCompleted { .. }) {
            break;
        }
    }
    handle.wait()?;

    let store = handle.store.lock().unwrap();
    print!("{}", QueryEngine::new(&store).summary());
    OkfExporter::new().export(&store, &config.output_path)?;
    Ok(())
}
```

Typical flow: `default_registry()` → `Pipeline::spawn` → observe `PipelineEvent`s / query `KnowledgeStore` → `OkfExporter::export`.

## Architecture

```text
gnosis (one crate)
├── binary: gnosis
└── library modules:
    ├── connectors, pipeline, store, query
    ├── providers (tree-sitter + docs/data + generic)
    ├── okf (export)
    └── tui (Ratatui UI)
```

Pipeline: Connector → Discovery → ProtoData → Provider selection → Analysis → Knowledge store → Query / OKF export.

Events flow over a bounded channel so the TUI observes progress without coupling to workers.

## What the PoC understands

- **Code:** C++, Rust, Elixir via Tree-sitter (modules, types, functions, imports/includes, inheritance where syntactic)
- **Docs:** Markdown, plain text
- **Data:** JSON, YAML, TOML, CSV (bounded)
- **Everything else:** honest unknown / partial with metadata and a candidate future provider category

## Limitations

- Connectors today: local filesystem and S3 (no remote Git hosts, web crawl)
- No LLM providers, vectors, or persistent index
- Tree-sitter is syntactic — not compiler-grade semantics
- Git enrichment uses the `git` CLI when available (filesystem scans only; not a pure-Rust Git library)
- OKF export is a hand-written markdown+YAML bundle plus `sidecar.json` (not the `okf` crates.io crate)
- No `gnosis.toml` yet — configuration is CLI flags only

## Development

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
cargo run -- scan ./fixtures/mixed-repo --no-tui
```

### CI/CD

| Workflow | Trigger | Purpose |
|----------|---------|---------|
| [ci.yml](.github/workflows/ci.yml) | push/PR to `main` | fmt, clippy, test (Linux/macOS/Windows), coverage → Coveralls, audit/deny, release build, crates.io package check |
| [release.yml](.github/workflows/release.yml) | `v*` tag | multi-platform binaries, GitHub Release, publish crates to crates.io |
| [dependencies.yml](.github/workflows/dependencies.yml) | weekly / manual | dependency update PRs |

Secrets / external setup:

- Enable the repo on [Coveralls](https://coveralls.io) (action uses `GITHUB_TOKEN`)
- Set `CARGO_REGISTRY_TOKEN` for crates.io publish on release tags

Releases: see [CONTRIBUTING.md](CONTRIBUTING.md#release-maintainers). Push an annotated `v*` tag matching `version` in `Cargo.toml` (publishes the single `gnosis` crate).

## Demo

See [demo.md](demo.md).

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for release history.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, PR expectations, and the maintainer release checklist.

## License

MIT — see [LICENSE](LICENSE).
