# CLI 出力仕様

## Scope

この文書は、`picofeedr` の CLI 出力契約を定義する。  
対象は `--output json` の envelope と、各コマンドの payload shape、`--output plain` の形式契約。

## Output Modes

- `--output json`: 機械可読契約
- `--output plain`: 人間向け表示

## JSON Envelope

`--output json` のとき、stdout は常に次の envelope で返す。

```json
// success / warning
{ "status": "ok|warning", "result": <payload>, "error": null, "meta": { "api_version": "<string>", "db_schema_version": <int>, "generated_at": <epoch> } }

// fatal
{ "status": "error", "result": null, "error": { "code": "<CODE>", "message": "<string>", "retryable": <bool>, "details": <object|null> }, "meta": { "api_version": "<string>", "db_schema_version": <int>, "generated_at": <epoch> } }
```

### Envelope Rules

- `status` は結果判定の単一軸
- `status = "error"` のとき `result = null` かつ `error != null` を必須とする
- `status in {"ok","warning"}` のとき `result != null` かつ `error = null` を必須とする
- `meta` は常に返す
- 致命失敗は exit code != 0
- `BrokenPipe` は下流終了として扱い、exit code 0 で終了する

## Command Payloads

### `version`

```json
{ "api_version": "<string>", "db_schema_version": <int>, "build": "<string>" }
```

- `db_schema_version` はこの binary の current schema version を返す

### `feeds`

```json
{ "feeds": [{ "feed_id": "<string>", "url": "<string>", "title": "<string|null>", "site_url": "<string|null>", "author": "<string|null>" }] }
```

`feeds` はローカル DB に記録済みの feed catalog 状態のみを返す。
`feeds.yaml` の load、validation、DB への feed 登録は行わない。

### `sync --check`

```json
{ "valid": <bool>, "errors": [ValidationIssue...], "warnings": [ValidationIssue...], "checked_feeds": <int>, "skipped_feeds": <int> }
```

`result.valid = false` のときは `status = "warning"` かつ exit code 1。

### `sync`

```json
{ "status": "completed|partial_failed|failed", "fetched_feed_count": <int>, "skipped_feed_count": <int>, "failed_feed_count": <int>, "new_entry_count": <int>, "duration_ms": <int>, "errors": [SyncError...] }
```

Blocking `feeds.yaml` validation errors fail the command with `status = "error"` and
`error.code = "CONFIG_ERROR"` before any fetch starts.

`SyncError` の shape:

```json
{ "feed_url": "<url>", "code": "FETCH_FAILED|PARSE_FAILED", "message": "<string>", "retryable": <bool> }
```

### `status`

```json
{ "revision": <int>, "last_write_at": <epoch|null>, "db_schema_version": <int>, "api_version": "<string>", "last_sync_at": <epoch|null>, "last_sync_status": "completed|partial_failed|failed|null" }
```

- `db_schema_version` は実 DB の current schema version を返す

### `list`

```json
{ "total_count": <int>, "items": [EntrySummary...], "feeds": [FeedSummary...], "next_page_token": "<token|null>", "revision": <int>, "last_write_at": <epoch|null>, "last_sync_at": <epoch|null> }
```

`EntrySummary` の最低契約:

```json
{ "entry_id": "<string>", "feed_id": "<string>", "title": "<string|null>", "link": "<string|null>", "published_at": "<epoch|null>", "first_seen_at": "<epoch>", "tags": ["..."] }
```

`FeedSummary` の shape:

```json
{ "feed_id": "<string>", "title": "<string|null>" }
```

### `view`

`result` は `EntryDetail`。  
最低でも `entry_id`, `feed_title`, `title`, `link` を含む。

`storage=db` の `content` が欠落している場合、および `storage=fs` の `ref` が欠落または不正な場合は `INTERNAL` エラーとする。妥当な `ref` に対応するファイルが欠落している場合は本文なしとして返す。
`storage=none` の場合は本文なし (`content: null`) として返す。

### `mark`

```json
{ "updated_entry_count": <int> }
```

### `tags`

```json
{ "tags": ["<tag>", "..."] }
```

## Plain Output Contract

`--output plain` は人間向け表示だが、行志向で読みやすく、単純なシェル加工にも使えることを目標にする。  
JSON ほど厳密な全文字列契約は持たないが、形式カテゴリと最低限の整形規則は契約として扱う。

### Plain Categories

| Category | Commands                                                     | Format                               |
| -------- | ------------------------------------------------------------ | ------------------------------------ |
| table    | `list`, `feeds`, `tags`                                      | タブ区切り、1レコード/行、ヘッダなし |
| kv       | `version`, `status`, `mark`, `view` metadata, `sync --check` | `key: value`、1行1項目               |
| log      | `sync`                                                       | `sync:* key=value ...`               |

### Common Rules

- `table` は 1 record = 1 line を守る
- `table` の列区切りはタブ (`\t`) を使う
- `table` の欠損値は空文字で表す
- `--id` のような任意列は末尾に追加する
- `kv` は `key: value` を使い、1行に複数項目を詰め込まない
- `kv` の null 値は文字列 `null` で表す
- `kv` の時刻はローカルタイムゾーンの ISO 8601 で表す
- `kv` で配列や繰り返し要素を表す場合は `field[index].subfield: value` 形式でフラット化する
- `log` は実行ログ向けで、1行ごとに自己完結した event を出す
- `log` の key/value は `key=value` を使う
- `log` で空白を含む文字列値を出す場合は引用符付きで表してよい

### `sync`

`sync` の plain 出力は log-oriented とする。default plain log では、ジョブ全体の開始、skip された feed、feed ごとの結果、最終要約を出す。`sync:start`、skip line、成功した feed ごとの結果、最終要約は stdout、詳細な error line は stderr に出す。

```text
sync:start total_feeds=<fetch-targets> skipped_feeds=<n>
sync:skip url=<feed_url> [feed_name=<json-string>]
sync:feed-ok index=<i>/<fetch-targets> url=<feed_url> entries=<n>
sync:done status=<completed|partial_failed|failed> fetched_feed_count=<n> skipped_feed_count=<n> failed_feed_count=<n> new_entry_count=<n> duration_ms=<n> errors=<n>
```

`sync:feed-ok` の `index=<i>/<fetch-targets>` は完了順の進捗カウンタであり、`i` はその sync で成功または失敗として処理が完了した feed の件数を表す。

詳細な error line は stderr に出す。

```text
sync:feed-error url=<feed_url> code=<FETCH_FAILED|PARSE_FAILED|INGEST_FAILED> retryable=<true|false> [feed_name=<json-string>] message="<text>"
```

### `list`

- 1エントリにつき1行を出力する
- 既定列は `datetime`, `title`, `feed_title`, `tags`, `link`
- `--id` 指定時は末尾列として `entry_id` を追加する
- `total_count` と `next_page_token` は stderr に出してよい

### `feeds`

- 1 feed につき1行を出力する
- 既定列は `title`, `url`, `site_url`, `author`
- `--id` 指定時は末尾列として `feed_id` を追加する
- `title` / `site_url` / `author` はローカル DB に記録された値を返す
- `feeds` コマンド自身は metadata refresh のための fetch を行わない
- `feeds` コマンド自身は `feeds.yaml` の feed を DB に登録しない

### `tags`

- 1 tag につき1行を出力する
- 単一列の `table` として扱う

### `view`

- metadata は `kv` 形式で出す
- `entry_id`, `title`, `feed_title`, `feed_id` は独立した項目として出す
- 本文がある場合は metadata block の後に空行を1つ出し、その後に raw text body をそのまま出す

### `sync --check`

- top-level summary は `kv` 形式で出す
- `errors` / `warnings` の詳細は診断行として出す
- 各診断は `error: code=<CODE> path=<PATH>` または `warning: code=<CODE> path=<PATH>` 形式を基本とする
- 補足説明がある場合は直後に `message: ...` 行を出してよい

## References

- エラー契約は `doc/spec/errors.md` を参照する
- JSON 命名規約は `doc/spec/api-naming.md` を参照する
- ページング契約は `doc/spec/pagination.md` を参照する
