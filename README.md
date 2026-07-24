# Gnosis

[![CI](https://github.com/thanos/gnosis/actions/workflows/ci.yml/badge.svg)](https://github.com/thanos/gnosis/actions/workflows/ci.yml)
[![Coverage Status](https://coveralls.io/repos/github/thanos/gnosis/badge.svg?branch=main)](https://coveralls.io/github/thanos/gnosis?branch=main)
[![crates.io](https://img.shields.io/crates/v/gnosis.svg)](https://crates.io/crates/gnosis)
[![docs.rs](https://docs.rs/gnosis/badge.svg)](https://docs.rs/gnosis)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)

**Gnosis compiles what you have into what you know.**

Gnosis is an enterprise knowledge compiler: a local-first Rust tool that discovers digital objects in a repository, extracts structured knowledge with deterministic providers (Tree-sitter and lightweight document/data parsers), shows its work in a live TUI, and exports an [OKF](https://github.com/GoogleCloudPlatform/knowledge-catalog) v0.1-style bundle.

This is a proof of concept — not a search engine, vector database, or chat UI.

## Install

### From crates.io

```bash
cargo install gnosis
```

### From source

```bash
cargo install --git https://github.com/thanos/gnosis
# or from a checkout:
cargo install --path crates/gnosis-cli
```

Prebuilt binaries for Linux, macOS, and Windows are attached to [GitHub Releases](https://github.com/thanos/gnosis/releases) on each `v*` tag (with `SHA256SUMS`).

## Quick start

```sh
cargo build --release -p gnosis
./target/release/gnosis scan ./fixtures/mixed-repo
```

Headless (CI / scripting):

```sh
cargo run -p gnosis -- scan ./fixtures/mixed-repo --no-tui --export
```

## Commands

| Command | Description |
|---------|-------------|
| `gnosis scan <path>` | Live TUI scan |
| `gnosis scan <path> --no-tui` | Headless scan + summary |
| `gnosis scan <path> --no-tui --export` | Also write `knowledge.okf/` |
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

Scanning is recursive under `<path>` (via `ignore::WalkBuilder`). Descent skips `.gitignore` matches, `target` / `node_modules` / `.git` / `knowledge.okf`, and does not follow symlinks.

### TUI commands (`:`)

`summary` · `objects [status]` · `unknown` · `providers` · `stats` · `find <text>` · `explain <name>` · `graph <name>` · `export okf [path]` · `quit`

Shortcuts: `s` summary · `u` unknown · `e` export · `q` quit

## Architecture

```text
crates/gnosis-cli  (package name: gnosis, binary: gnosis)
     ↓
gnosis-tui
     ↓
gnosis-core   (types, connectors, pipeline, store, query)
     ↓
gnosis-providers  (tree-sitter C++/Rust/Elixir + md/text/json/yaml/toml/csv + generic)
     ↓
gnosis-okf    (OKF export behind Exporter trait)
```

Pipeline: Connector → Discovery → ProtoData → Provider selection → Analysis → Knowledge store → Query / OKF export.

Events flow over a bounded channel so the TUI observes progress without coupling to workers.

## What the PoC understands

- **Code:** C++, Rust, Elixir via Tree-sitter (modules, types, functions, imports/includes, inheritance where syntactic)
- **Docs:** Markdown, plain text
- **Data:** JSON, YAML, TOML, CSV (bounded)
- **Everything else:** honest unknown / partial with metadata and a candidate future provider category

## Limitations

- Local filesystem only (no S3, remote Git hosts, web crawl)
- No LLM providers, vectors, or persistent index
- Tree-sitter is syntactic — not compiler-grade semantics
- Git enrichment uses the `git` CLI when available (not a pure-Rust Git library)
- OKF export is a hand-written markdown+YAML bundle plus `sidecar.json` (not the `okf` crates.io crate)
- No `gnosis.toml` yet — configuration is CLI flags only

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
cargo run -p gnosis -- scan ./fixtures/mixed-repo --no-tui
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

Releases: push a `v*` tag (e.g. `v0.1.0`) matching `workspace.package.version`.

## Demo

See [demo.md](demo.md).

## License

MIT — see [LICENSE](LICENSE).
