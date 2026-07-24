# Gnosis

**Gnosis compiles what you have into what you know.**

Gnosis is an enterprise knowledge compiler: a local-first Rust tool that discovers digital objects in a repository, extracts structured knowledge with deterministic providers (Tree-sitter and lightweight document/data parsers), shows its work in a live TUI, and exports an [OKF](https://github.com/GoogleCloudPlatform/knowledge-catalog) v0.1-style bundle.

This is a proof of concept — not a search engine, vector database, or chat UI.

## Quick start

```sh
cargo build --release
./target/release/gnosis scan ./fixtures/mixed-repo
```

Headless (CI / scripting):

```sh
cargo run -p gnosis-cli -- scan ./fixtures/mixed-repo --no-tui --export
```

## Commands

| Command | Description |
|---------|-------------|
| `gnosis scan <path>` | Live TUI scan |
| `gnosis scan <path> --no-tui` | Headless scan + summary |
| `gnosis scan <path> --no-tui --export` | Also write `knowledge.okf/` |
| `gnosis about` | Product overview |

### TUI commands (`:`)

`summary` · `objects [status]` · `unknown` · `providers` · `stats` · `find <text>` · `explain <name>` · `graph <name>` · `export okf [path]` · `quit`

Shortcuts: `s` summary · `u` unknown · `e` export · `q` quit

## Architecture

```text
gnosis-cli  →  gnosis-tui
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
- Git enrichment uses the `git` CLI when available
- OKF export is a pragmatic markdown+YAML bundle with a `sidecar.json` for extra provenance

## Demo

See [demo.md](demo.md).

## License

MIT
