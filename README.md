# picofeedr

`picofeedr` is a CLI tool for collecting RSS/Atom feeds into a local store, browsing saved entries, and managing tags on them.
It is built for people who want to keep feed data on their own machine and work with it from the terminal or from scripts.

Status: alpha. Command behavior, response schemas, and configuration details may still change.

## What Is picofeedr?

`picofeedr` is a small feed ingestion and tagging tool for local use.
At a glance, it lets you:

- fetch and ingest configured feeds
- persist entries locally
- manage unread and custom tags on entries
- inspect results in the terminal or consume them as JSON

The main use case is simple: sync feeds on your machine, keep a persistent archive of entries, then search, inspect, and tag them later.

## Installation

The repository includes a pinned Rust toolchain via `rust-toolchain.toml`.
With `rustup`, the expected toolchain is selected automatically.

Build from source:

```bash
cargo build --release
```

The binary will be available at:

```text
target/release/picofeedr
```

You can also install it into Cargo's binary directory:

```bash
cargo install --path .
```

## Quick Start

Create a minimal `config.toml`:

```toml
unread_tag = "unread"

[storage]
root_dir = "./var/picofeedr"
content_store = "db"

[feeds]
source = "./feeds.yaml"

[cli]
output = "plain"
```

Relative paths such as `./feeds.yaml` and `./var/picofeedr` are resolved from the current working directory when you run `picofeedr`.

Create a minimal `feeds.yaml`:

```yaml
picofeedr:
  tech:
    tags: [tech]
    feeds:
      - title: Example Feed
        url: https://example.com/feed.xml
```

Sync feeds:

```bash
target/release/picofeedr --config ./config.toml sync
```

Typical `plain` progress output:

```text
sync:start total_feeds=1
sync:feed start index=1/1 url=https://example.com/feed.xml
sync:feed ok index=1/1 url=https://example.com/feed.xml entries=3
status: completed
fetched_feed_count: 1 failed_feed_count: 0 new_entry_count: 3 duration_ms: 120
```

List recent entries:

```bash
target/release/picofeedr --config ./config.toml list --sort date-desc --limit 5
```

`plain` list output is tab-separated:

```text
2026-03-19T09:30:00+09:00	First Entry	Example Feed	unread, tech	https://example.com/1
2026-03-19T08:00:00+09:00	Second Entry	Example Feed	unread, tech	https://example.com/2
```

Metadata such as `total_count` and `next_page_token` is written to stderr in `plain` mode.

View one entry in detail:

```bash
target/release/picofeedr --config ./config.toml view <entry-id>
```

Query in JSON for automation:

```bash
target/release/picofeedr --config ./config.toml --output json list --query 'tag:tech after:1w' | jq '.result.items[].title'
```

Storage can be configured in three ways:

- keep metadata and content together in SQLite
- keep metadata in SQLite and store content bodies in a filesystem directory
- keep entry metadata and tags, but do not store feed-provided content bodies

## Common Tasks

If you are just getting started, these are the commands you will usually use first:

| Task | Command |
| --- | --- |
| Validate your feed configuration | `feeds --config-check` |
| Fetch latest entries | `sync` |
| List saved entries | `list` |
| View one saved entry | `view <entry-id>` |
| Mark entries read or unread | `mark read ...` / `mark unread ...` |
| Add or remove custom tags | `mark tag ...` |

## Commands

| Command | Description |
| --- | --- |
| `feeds` | List feeds or run static config validation |
| `sync` | Sync feeds and ingest new entries |
| `list` | List entry summaries |
| `view <entry-id>` | View entry detail by id |
| `mark read <entry-id>...` | Remove the unread tag from entries |
| `mark unread <entry-id>...` | Add the unread tag to entries |
| `mark tag ...` | Add or remove custom tags |
| `tags` | List tags stored in the database |
| `status` | Show lightweight database status metadata |
| `ping` | Print a simple health response |
| `version` | Print version information |

Examples:

```bash
target/release/picofeedr --config ./config.toml feeds --config-check
target/release/picofeedr --config ./config.toml --output plain list --query 'tag:tech after:1w'
target/release/picofeedr --config ./config.toml --output json list | jq '.result.items[].title'
target/release/picofeedr --config ./config.toml mark read <entry-id>
```

For full command-line details, run `picofeedr --help` or see [`doc/spec/cli.md`](doc/spec/cli.md).

## Configuration

`picofeedr` uses two main files:

- `config.toml` for CLI behavior and storage settings
- `feeds.yaml` for feed definitions, grouping, inherited tags, and auto-tag rules

Important `config.toml` keys:

| Key | Purpose |
| --- | --- |
| `unread_tag` | Tag used by `mark read` / `mark unread` |
| `storage.root_dir` | Root directory containing `db.sqlite` and optional stored content |
| `storage.content_store` | Content storage mode: `db`, `fs`, or `none` |
| `feeds.source` | Path to `feeds.yaml` |
| `cli.output` | Default output mode: `plain` or `json` |
| `query.default_limit` | Default result limit for `list` |
| `query.max_limit` | Maximum allowed limit for `list` |

Example `feeds.yaml` with groups and auto-tag rules:

```yaml
picofeedr:
  auto_tags:
    - title_contains: [security, cve]
      add_tags: [security-alert]
      priority: 10

  tech:
    tags: [tech]
    auto_tags:
      - title_contains: [rust]
        add_tags: [rust]
    feeds:
      - title: Platform Blog
        url: https://feeds.example.invalid/tech/platform.xml
```

Detailed specifications:

- [`doc/spec/config.md`](doc/spec/config.md)
- [`doc/spec/feeds.md`](doc/spec/feeds.md)
- [`doc/spec/feeds.sample.yaml`](doc/spec/feeds.sample.yaml)

## Output Formats

Choose `plain` when you are working interactively in the terminal.
Choose `json` when you want to pipe results into `jq`, scripts, or other tools.

`plain` output:

- optimized for terminal inspection
- `list` writes tab-separated rows
- `status` renders timestamps in local time
- `sync` prints incremental progress lines and a final summary

`json` output:

- is the main contract for automation
- wraps responses in a consistent envelope with `status`, `result`, `error`, and `meta`
- is covered by JSON Schema documents in [`doc/spec/schema`](doc/spec/schema)

Schema files are available for `config-check`, `feeds`, `list`, `mark`, `ping`, `status`, `sync`, `tags`, `version`, `view`, and fatal error responses.

## Development

Common commands:

```bash
cargo fmt -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo run --bin schemas
git diff --exit-code -- doc/spec/schema
cargo test
```

Optional local fast loop:

```bash
cargo fmt
cargo test
```

Optional benchmark:

```bash
cargo bench --bench tag_query_complex
```

Project layout:

- `src/` application source
- `tests/` integration and CLI tests
- `doc/spec/` specifications and schemas

## License

MIT License.
