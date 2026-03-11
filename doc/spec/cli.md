# CLI 出力仕様

## Scope

この文書は、`picofeedr` の CLI 出力契約を定義する。  
対象は `--output json` の envelope と、各コマンドの payload shape、`--output plain` の最低契約。

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

### `ping`

```json
{ "status": "ok", "result": { "status": "ok" }, "error": null, "meta": { ... } }
```

### `version`

```json
{ "api_version": "<string>", "db_schema_version": <int>, "build": "<string>" }
```

### `feeds`

```json
{ "feeds": [{ "feed_id": "<string>", "url": "<string>", "title": "<string|null>", "site_url": "<string|null>", "author": "<string|null>", "tags": ["..."] }] }
```

### `feeds --config-check`

```json
{ "valid": <bool>, "errors": [ValidationIssue...], "warnings": [ValidationIssue...], "checked_feeds": <int> }
```

`result.valid = false` のときは `status = "warning"` かつ exit code 1。

### `sync`

```json
{ "status": "completed|partial_failed|failed", "fetched_feed_count": <int>, "failed_feed_count": <int>, "new_entry_count": <int>, "duration_ms": <int>, "errors": [SyncError...] }
```

`SyncError` の shape:

```json
{ "feed_url": "<url>", "code": "FETCH_FAILED|PARSE_FAILED", "message": "<string>", "retryable": <bool> }
```

### `status`

```json
{ "revision": <int>, "last_write_at": <epoch|null>, "db_schema_version": <int>, "api_version": "<string>", "last_sync_at": <epoch|null>, "last_sync_status": "completed|partial_failed|failed|null" }
```

### `list`

```json
{ "total_count": <int>, "items": [EntrySummary...], "feeds": [FeedSummary...], "next_page_token": "<token|null>", "revision": <int>, "last_write_at": <epoch|null> }
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

### `mark`

```json
{ "updated_entry_count": <int> }
```

### `tags`

```json
{ "tags": ["<tag>", "..."] }
```

## Plain Output Minimum Contract

`--output plain` は人間向け表示で、JSONほど厳密な全文字列契約は持たない。  
ただし次は契約として扱う。

### `sync`

実行中に feed 単位の進捗を stdout に逐次出力する。

```text
sync:start total_feeds=<N>
sync:feed start index=<i>/<N> url=<feed_url>
sync:feed ok index=<i>/<N> url=<feed_url> entries=<k>
sync:feed error index=<i>/<N> url=<feed_url> code=<FETCH_FAILED|PARSE_FAILED> retryable=<true|false>
```

進捗行の後に最終サマリを出す。

### `list`

- 1エントリにつき1行を出力する
- `--id` 指定時は末尾列として `entry_id` を追加する
- `total_count` と `next_page_token` は stderr に出してよい

### `status`

- `last_write_at` / `last_sync_at` は人間可読なローカル時刻で表示してよい

## References

- エラー契約は `doc/spec/errors.md` を参照する
- JSON 命名規約は `doc/spec/api-naming.md` を参照する
- ページング契約は `doc/spec/pagination.md` を参照する
