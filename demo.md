# Gnosis PoC demo script

No hidden setup beyond a Rust toolchain (`cargo` / `rustc`). Optional: `git` on `PATH` for repository enrichment.

## 1. Build

```sh
cd /path/to/gnosis
cargo build
```

## 2. Headless scan of the fixture repo

```sh
cargo run -- scan ./fixtures/mixed-repo --no-tui --quiet --export --output /tmp/gnosis-demo.okf
```

Expect:

- Summary counts for objects / understood / partial / unknown (fixture typically ~12 objects, 10 understood, 1 partial, 1 unknown)
- Entities such as C++ `PricingEngine` / `RuleSet`, Rust `Catalog`, Elixir `Pricing.Engine`
- `data/blob.bin` listed under unknown/partial
- `ignored/secret.bin` absent (fixture `.gitignore`)
- OKF bundle at `/tmp/gnosis-demo.okf` with `index.md`, `entities/`, `objects/`, `relationships/`, `sidecar.json`

## 3. Live TUI (optional)

```sh
cargo run -- scan ./fixtures/mixed-repo
```

Watch objects appear, then try:

- `:summary`
- `:find PricingEngine`
- `:explain Catalog`
- `:graph PricingEngine`
- `:unknown`
- `:export okf /tmp/gnosis-tui.okf`
- `q`

## 4. Integrity check

Confirm source files under `fixtures/mixed-repo` were not modified (read-only scan).
