# Gnosis PoC demo script

No hidden setup beyond a Rust toolchain (`cargo` / `rustc`).

## 1. Build

```sh
cd /path/to/gnosis
cargo build -p gnosis-cli
```

## 2. Headless scan of the fixture repo

```sh
cargo run -p gnosis-cli -- scan ./fixtures/mixed-repo --no-tui --export --output /tmp/gnosis-demo.okf
```

Expect:

- Summary counts for objects / understood / partial / unknown
- C++ `PricingEngine`, Rust `Catalog`, Elixir `Pricing.Engine` among entities
- `data/blob.bin` listed under unknown/partial
- `ignored/secret.bin` absent (`.gitignore`)
- OKF bundle at `/tmp/gnosis-demo.okf` with `index.md`, `entities/`, `objects/`, `sidecar.json`

## 3. Live TUI (optional)

```sh
cargo run -p gnosis-cli -- scan ./fixtures/mixed-repo
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
