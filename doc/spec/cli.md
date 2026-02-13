# CLI API（出力）

## A6. CLI API（出力）

### A6.1 共通

CLI の主要な出力は stdout に出すのだ。`--output` で形式を切り替えるのだ。

```
--output json   # 機械可読（UI/自動化向け）
--output plain  # 対話向け
```

#### A6.1.1 JSON モードの共通 envelope（v1）

`--output json` のとき、stdout は常にこの共通形式で包むのだ。

```json
// success
{ "success": true,  "result": <payload>, "error": null, "meta": {"api_version": "<string>", "schema_version": <int>, "generated_at": <epoch>} }

// fatal
{ "success": false, "result": null,      "error": { "code": "<CODE>", "message": "<string>", "retriable": <bool>, "details": <object|null> }, "meta": {"api_version": "<string>", "schema_version": <int>, "generated_at": <epoch>} }
```

`result` の中身（payload）はコマンドごとに定義するのだ。致命では `error` を埋め、exit code も !=0 にするのだ（詳細は `doc/spec/errors.md`）。
`stdout` が `BrokenPipe` になった場合は、下流コマンドの早期終了とみなして非致命（exit code 0）で終了するのだ。

```
picofeedr version
# → {"success": true, "result": {"api_version": "0.5.0", "schema_version": 1, "build": "dev"}, "error": null, "meta": {...}}

picofeedr ping
# → {"success": true, "result": {"status": "ok"}, "error": null, "meta": {...}}
```

### A6.2 フィード管理

```
picofeedr feeds
# → {"success": true, "result": {"feeds": [{id, feed_key, url, title, site_url, author, tags}]}, "error": null, "meta": {...}}

picofeedr feeds --config-check
# → {"success": true, "result": {"valid": true, "errors": [], "warnings": [], "checked_feeds": 12}, "error": null, "meta": {...}}
```

**ConfigCheckResult：**

```json
{ "valid": <bool>, "errors": [ValidationIssue...], "warnings": [ValidationIssue...], "checked_feeds": <int> }
```

### A6.3 同期（取得）

```
picofeedr sync
# → {"success": true, "result": {"status": "completed", "fetched_feed_count": 120, "failed_feed_count": 0, "new_entry_count": 42, "duration_ms": 245300, "errors": []}, "error": null, "meta": {...}}

# 一部失敗時の例
# → {"success": true, "result": {"status": "partial_failed", "fetched_feed_count": 120, "failed_feed_count": 3, "new_entry_count": 42, "duration_ms": 245300, "errors": [{"feed_url": "...", "code": "FETCH_FAILED", "message": "...", "retriable": true}]}, "error": null, "meta": {...}}
```

**SyncResult：**

```json
{ "status": "completed|partial_failed|failed", "fetched_feed_count": <int>, "failed_feed_count": <int>, "new_entry_count": <int>, "duration_ms": <int>, "errors": [SyncError...] }
```

**SyncError：**

```json
{ "feed_url": "<url>", "code": "FETCH_FAILED|PARSE_FAILED", "message": "<string>", "retriable": <bool> }
```

### A6.4 DB状態メタデータ（軽量）

```
picofeedr status
# → {"success": true, "result": {"revision": 1284, "last_write_at": 1705420900, "schema_version": 1, "api_version": "0.5.0", "last_sync_at": 1705420800, "last_sync_status": "completed"}, "error": null, "meta": {...}}
```

**StatusResponse：**

```json
{ "revision": <int>, "last_write_at": <epoch|null>, "schema_version": <int>, "api_version": "<string>", "last_sync_at": <epoch|null>, "last_sync_status": "completed|partial_failed|failed|null" }
```

### A6.5 一覧検索（軽量メタデータのみ）

```
picofeedr list --query <q> --sort <date_desc|date_asc|first_seen_desc|first_seen_asc> --limit <n> [--cursor <cursor>]
# → {"success": true, "result": {"total_count": 342, "items": [EntrySummary...], "next_page_token": "eyJ...", "revision": 1284, "last_write_at": 1705420900}, "error": null, "meta": {...}}
```

**ListResponse：**

```json
{ "total_count": <int>, "items": [EntrySummary...], "next_page_token": "<token|null>", "revision": <int>, "last_write_at": <epoch|null> }
```

### A6.6 詳細取得（遅延）

```
picofeedr view <id>
# → {"success": true, "result": EntryDetail, "error": null, "meta": {...}}
```

### A6.7 状態更新

```
picofeedr mark read ...
# → {"success": true, "result": {"updated_entry_count": 2}, "error": null, "meta": {...}}
```

### A6.8 タグ

```
picofeedr tags
# → {"success": true, "result": {"tags": ["unread", "tech", "security", "rust", ...]}, "error": null, "meta": {...}}
```
