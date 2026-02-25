# picofeedr

`picofeedr` is a lightweight feed ingestion and search CLI built with Rust.
It fetches feed entries, normalizes them, stores them in SQLite, and provides query-oriented output for automation and tooling.

## Features

- Fetch and ingest feed entries from configured sources
- Normalize and persist entries into SQLite
- Query entries with structured CLI commands
- JSON-friendly output for scripting and integration
- Modular Rust codebase with focused domain layers (`feed`, `sync`, `db`, `query`)

## Requirements

- Rust toolchain (recommended: stable via `rustup`)
- SQLite (bundled through `rusqlite` with the `bundled` feature)

## Installation

```bash
cargo build --release
```

Binary output:

```text
target/release/picofeedr
```

## Quick Start

1. Build the project.
2. Prepare your configuration files.
3. Run the CLI command you need.

Example:

```bash
cargo run -- --help
```

## Development

Run tests:

```bash
cargo test
```

Run complex tag-query benchmarks (10k/50k/100k datasets):

```bash
cargo bench --bench tag_query_complex
```

Run formatter:

```bash
cargo fmt
```

Run lint checks:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Generate JSON Schema artifacts:

```bash
cargo run --bin schemas
```

## PR Checklist

- [ ] `cargo fmt`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test`
- [ ] `cargo run --bin schemas` and commit `doc/spec/schema/*.schema.json`

## Project Structure

- `src/`: application source code
- `tests/`: integration and CLI tests
- `doc/`: specifications and design documents

## License

MIT License.
