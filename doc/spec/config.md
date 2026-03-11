# 設定仕様

## Scope

この文書は、`picofeedr` が読み込む設定ファイルの責務分担と、`config.toml` の契約を定義する。  
`feeds.yaml` の詳細な読み込み仕様と検証仕様は `doc/spec/feeds.md` を正本とする。

## Files

- `config.toml`: CLI動作設定
- `feeds.yaml`: フィード定義と自動タグ規則

## `config.toml` Contract

`config.toml` はアプリケーションの動作方法を定義する。  
CLIフラグが同等の設定項目を持つ場合、CLIフラグを優先する。

### Example

```toml
unread_tag = "unread"

[storage]
root_dir = "~/.local/share/picofeedr"
content_store = "db"

[sync]
parallel = 5
timeout = 30
user_agent = "picofeedr/0.1.0"
retry_count = 3
retry_delay = 5

[query]
default_limit = 100
max_limit = 1000

[feeds]
source = "~/.config/picofeedr/feeds.yaml"

[cli]
output = "plain"

[log]
level = "info"
```

### Top-Level Keys

- `unread_tag: string`

### `[storage]`

- `root_dir: path`
- `content_store: "db" | "fs" | "none"`

`storage.root_dir` から `db.sqlite` と `data/` を導出する。  
DBパスは `storage.root_dir/db.sqlite`、ファイル保存先は `storage.root_dir/data`。

`content_store = "fs"` のとき、`entry_contents.ref` は hash key を保持し、保存パスは `storage.root_dir/data` とアプリ側の導出規則で決まる。  
レコードに実パスは保存しない。

### `[sync]`

- `parallel: integer`
- `timeout: integer`
- `max_feed_bytes: integer`（省略時は実装既定値）
- `user_agent: string`
- `retry_count: integer`
- `retry_delay: integer`

### `[query]`

- `default_limit: integer`
- `max_limit: integer`

`default_limit` / `max_limit` はどちらも 1 以上を必須とする。  
`default_limit <= max_limit` を必須とする。  
`list --limit` が `max_limit` を超える場合は `INVALID_QUERY`。

### `[feeds]`

- `source: path`

`feeds.source` は `feeds.yaml` のパス。

### `[cli]`

- `output: "json" | "plain"`

`cli.output` は既定出力形式。  
詳細な出力契約は `doc/spec/cli.md` を正本とする。

### `[log]`

- `level: "error" | "warn" | "info" | "debug" | "trace"`

ログや診断出力は原則 stderr に寄せる。

## `feeds.yaml` Contract Boundary

- `feeds.yaml` は取得対象、タグ継承、自動タグ規則を定義する
- `feeds.yaml` が購読定義の source of truth
- `config.toml` は `feeds.yaml` の中身を上書きしない

## Non-Goals

- この文書は `feeds.yaml` の詳細な木構造や validation code を定義しない
- この文書は CLI出力形式を定義しない
