# Contributing to Gnosis

Thanks for helping improve Gnosis.

## Development setup

Requirements:

- Rust **1.85+** (edition 2021)
- Optional: `git` on `PATH` for repository enrichment during scans

```bash
git clone https://github.com/thanos/gnosis.git
cd gnosis
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo deny check
```

Headless demo against the fixture:

```bash
cargo run -p gnosis -- scan ./fixtures/mixed-repo --no-tui --quiet
```

## Project layout

| Path | Role |
|------|------|
| `crates/gnosis-cli/` | Package `gnosis` — CLI binary |
| `crates/gnosis-core/` | Pipeline, connectors, store, query |
| `crates/gnosis-providers/` | Understanding providers |
| `crates/gnosis-okf/` | OKF-style exporter |
| `crates/gnosis-tui/` | Ratatui UI |
| `fixtures/mixed-repo/` | Demo / e2e fixture |
| `.github/workflows/` | CI, release, dependency updates |

## Coding guidelines

- Keep connectors, providers, presentation, and OKF export independent
- Prefer deterministic providers; do not make an LLM mandatory
- Be honest about unknown / partial / failed understanding
- Match existing module style; avoid drive-by refactors
- Run `cargo deny check` when changing dependencies

## Pull requests

1. Open a PR against `main`
2. Ensure CI is green (fmt, clippy, tests, coverage gate ≥80%)
3. Update `CHANGELOG.md` under `[Unreleased]` when user-facing behavior changes
4. Keep the diff focused on one concern

## Release (maintainers)

See [RELEASE.md](RELEASE.md) for the full checklist. Summary:
1. Ensure `workspace.package.version` in root `Cargo.toml` matches the release (e.g. `0.1.0`)
2. Move `[Unreleased]` notes into a dated section in `CHANGELOG.md`
3. Merge to `main`
4. Ensure repository secrets:
   - `CARGO_REGISTRY_TOKEN` — crates.io API token (`cio_…` value only; no quotes / Bearer / trailing newline)
   - Coveralls enabled for the GitHub repo (uses `GITHUB_TOKEN`)
5. Tag and push:

   ```bash
   git tag -a v0.1.0 -m "v0.1.0"
   git push origin v0.1.0
   ```

6. The [Release](.github/workflows/release.yml) workflow builds binaries, creates a GitHub Release with `SHA256SUMS`, and publishes crates in dependency order:
   `gnosis-core` → `gnosis-providers` → `gnosis-okf` → `gnosis-tui` → `gnosis`

Manual dry-run (leaf crate):

```bash
cargo publish -p gnosis-core --dry-run
```
