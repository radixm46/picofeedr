# picofeedr

`picofeedr` is a CLI tool for collecting RSS/Atom feeds into a local store, browsing saved entries, and managing tags on them.
It is built for people who want to keep feed data on their own machine and work with it from the terminal or from scripts.

## Features

- **Local-first**: Network access only during `sync`. All reading and state updates happen locally via SQLite
- **Tag-centric design**: All state including unread is managed through tags, with unread tracking optionally disabled via config
- **Config-driven**: Feed definitions and auto-tagging rules managed in a single `feeds.yaml`
- **Multiple protocols**: Supports HTTP, HTTPS, Gopher, and local files (`file://`)
- **CLI-based**: Works standalone or as a backend for other tools (e.g., Emacs)
- **Machine-readable output**: `--output json` provides JSON Schema-compliant output

## Installation

```bash
cargo install --path .
```

Or build from source:

```bash
cargo build --release
```

## Quick Start

1. Create config directory:

```bash
mkdir -p ~/.config/picofeedr
```

2. Create `~/.config/picofeedr/feeds.yaml`:

```yaml
picofeedr:
  tech:
    tags: [tech]
    feeds:
      - title: Example Blog
        url: https://example.com/feed.xml
```

3. Sync feeds:

```bash
picofeedr sync
```

4. List entries:

```bash
picofeedr list --query unread
```

`config.toml` is optional. If omitted, `picofeedr` uses `~/.config/picofeedr/feeds.yaml` and stores data under `~/.local/share/picofeedr`.

## Commands

| Command                          | Description                      |
| -------------------------------- | -------------------------------- |
| `sync`                           | Sync feeds                       |
| `sync --check`                   | Validate config (no DB required) |
| `list [--query <q>]`             | List entries (see options below) |
| `view <id>`                      | View entry details               |
| `mark read <ids>`                | Mark as read                     |
| `mark unread <ids>`              | Mark as unread                   |
| `mark tag <ids> --add <tags>`    | Add tags (comma-separated)       |
| `mark tag <ids> --remove <tags>` | Remove tags (comma-separated)    |
| `tags`                           | List tags                        |
| `feeds [--id]`                   | List feeds                       |
| `status`                         | Show DB status metadata          |
| `version`                        | Print version information        |

When `manage_unread = false`, automatic unread-tag assignment is disabled, but `unread` queries and `mark read` / `mark unread` still work as aliases for `unread_tag`.

### Query Syntax

```bash
picofeedr list --query 'unread'
picofeedr list --query 'tag:(rust & cli)'
picofeedr list --query 'after:1w'
picofeedr list --query 'foo -bar'
picofeedr list --query '"quoted term"'
picofeedr list --query '(color|colour) -(draft|sponsored)'
```

Supported terms:
- `unread` - unread entries (`tag:<unread_tag>` shorthand)
- `tag:<expr>` - tag expression: `AND` / `OR` / `NOT`, `&` / `|` / `!`, and `()`
- `-tag:<expr>` - exclude tags
- `<term>` or `"<term>"` - title search term
- `-<term>` or `-"<term>"` - exclude title search term
- `(<expr>)` - title term expression group
- `-(<expr>)` - exclude title term expression group
- `feed:<id>` or `feed:"<title>"` - filter by feed
- `after:<YYYY-MM-DD|Nd|Nw|Nm|Ny>`, `before:<YYYY-MM-DD|Nd|Nw|Nm|Ny>` - date range

### List Options

```
--sort <order>   Sort order: first-seen-desc (default) | first-seen-asc |
                 date-desc | date-asc
--limit <n>      Number of entries per page (default: query.default_limit)
--cursor <token> Pagination cursor from the previous page
--id             Append entry id as the last column in plain output
```

### Common Flags

```
--config <path>       Path to config.toml
--storage-root <path> Override storage root directory
--output <json|plain> Output format (default: plain)
--debug               Enable debug output on stderr
```

## Configuration

See `doc/spec/config.md` and `doc/spec/feeds.md` for details.

### config.toml

`config.toml` is optional. All sections have sensible defaults.

```toml
# Storage location (default: ~/.local/share/picofeedr)
# [storage]
# root_dir = "~/.local/share/picofeedr"
# content_store = "db"  # "db" | "fs" | "none"

# Feed definitions (default: ~/.config/picofeedr/feeds.yaml)
# [feeds]
# source = "~/.config/picofeedr/feeds.yaml"

# Whether unread tracking is enabled (default: true)
# manage_unread = true

# Tag name for unread entries (default: "unread")
# unread_tag = "unread"

# Sync behavior
# [sync]
# parallel = 5
# timeout = 30             # seconds
# max_feed_bytes = 2097152 # bytes (default: 2MiB)
# retry_count = 3
# retry_delay = 5          # seconds

# List command defaults
# [query]
# default_limit = 100      # entries per page
# max_limit = 1000         # hard cap for --limit

# Output format (default: "plain")
# [cli]
# output = "plain"  # "plain" | "json"
```

### feeds.yaml

```yaml
picofeedr:
  auto_tags:
    - title_contains: [security, cve]
      add_tags: [security-alert]

  news:
    tags: [news]
    feeds:
      - title: Daily Bulletin
        url: https://example.com/news.xml

  tech:
    tags: [tech]
    dev:
      tags: [dev]
      feeds:
        - title: Compiler Notes
          url: https://example.com/dev.xml
```

## JSON Schema

JSON Schema for CLI output is available in `schemas/`. To regenerate:

```bash
cargo run --bin schemas
```

## Development

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

## License

MIT
