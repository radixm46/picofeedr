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

Run formatter:

```bash
cargo fmt
```

Run lint checks:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

## Project Structure

- `src/`: application source code
- `tests/`: integration and CLI tests
- `doc/`: specifications and design documents

## License

MIT License.
