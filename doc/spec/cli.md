# CLI API（出力）

## A6. CLI API（出力）

### A6.1 共通

CLI の主要な出力は stdout に出すのだ。`--output` で形式を切り替えるのだ。

```
--output json   # 機械可読（UI/自動化向け）
--output plain  # 対話向け
```

#### A6.1.1 JSON モードの共通envelope

`--output json` のとき、stdout は常にこの共通形式で包むのだ。

```
// success
{ "ok": true,  "data": <payload>, "error": null }

// fatal
{ "ok": false, "data": null,      "error": { "code": "<CODE>", "message": "<string>", "retry": <bool> } }
```

`data` の中身（payload）はコマンドごとに定義するのだ。致命では `error` を埋め、exit code も !=0 にするのだ（詳細は `doc/spec/errors.md`）。

**TODO（将来拡張の候補）：**

- `meta` フィールド（例: `request_id`, `elapsed_ms`, `api_version`, `schema_version`）を追加して、UI/自動化側の観測性を上げるのだ。
- `error.data`（機械可読な補助情報）と `error.causes`（エラーチェーン）を追加して、復旧/表示分岐を強化するのだ。

```
picofeedr version
# → {"ok": true, "data": {"api_version": "0.5.0", "schema_version": 1, "build": "abc123"}, "error": null}

picofeedr ping
# → {"ok": true, "data": {"ok": true}, "error": null}

```

### A6.2 フィード管理

```
picofeedr feeds
# → {"ok": true, "data": {"feeds": [{id, feed_key, url, title, site_url, author, tags}]}, "error": null}

picofeedr feeds --config-check
# → {"ok": true, "data": {"valid": true, "errors": [], "warnings": [], "checked_feeds": 12}, "error": null}

```

**注意：**

* フィードの追加/削除は `feeds.yaml` を直接編集
* `sync` 実行時に自動的にDBと同期される
* `feeds` の `tags` は `feeds.yaml` 由来の情報であり、DBの正本ではない（DBは購読の真実を保持しない）
* `feeds --config-check` は **静的検証専用** で、DB差分表示は行わないのだ

**ConfigCheckResult：**

```
{ "valid": <bool>, "errors": [ValidationIssue...], "warnings": [ValidationIssue...], "checked_feeds": <int> }
```

**ValidationIssue：**

```
{ "code": "DUPLICATE_FEED_URL|EMPTY_FEED_URL|INVALID_AUTO_TAG_RULE|...", "message": "<string>", "path": "<feeds.yaml logical path>|null" }
```

**終了コード：**

* `valid=true` は exit code 0
* `valid=false` は exit code 1
* warning のみ（`errors=[]`）は exit code 0

### A6.3 同期（取得）

```
picofeedr sync
# → {"ok": true, "data": {"status": "completed", "fetched": 120, "failed": 0, "new_entries": 42, "elapsed": 245.3, "errors": []}, "error": null}

# 一部失敗時の例
# → {"ok": true, "data": {"status": "partial_failed", "fetched": 120, "failed": 3, "new_entries": 42, "elapsed": 245.3, "errors": [{"feed_url": "...", "code": "FETCH_FAILED", "message": "...", "retry": true}]}, "error": null}
```

**SyncResult：**

```
{ "status": "completed|partial_failed|failed", "fetched": <int>, "failed": <int>, "new_entries": <int>, "elapsed": <float>, "errors": [SyncError...] }
```

**SyncError：**

```
{ "feed_url": "<url>", "code": "FETCH_FAILED|PARSE_FAILED", "message": "<string>", "retry": <bool> }
```

**挙動：**

* 取得失敗が一部に留まる場合は **exit code 0** とし、`status=partial_failed` + `errors` に詳細を載せる
* 取得失敗が **全件** の場合は **exit code 0** のまま `status=failed` とする
* DB書き込み失敗など **永続化に影響する失敗** は **致命** とする（A10のエラーJSONで終了、exit code != 0）
* 設定エラーなど **致命的な失敗** は A10 のエラーJSONで終了する（exit code != 0）
* `failed` は `errors` の件数と一致する

**sync の動作フロー：**

1. `feeds.yaml` を読み込み、`feeds` 配下を階層フラット化（タグ継承・`feeds.auto_tags` をコンパイル）
2. `feeds` カタログを upsert（`feed_key` を算出し、`url`/`title`/`site_url`/`author`/`meta_json` を更新）
   - `meta_json` は拡張メタ用途であり、`feeds.yaml` の tags / ルールは保存しないのだ
   - YAMLから削除されたURL → **何もしない**（履歴保持。同期対象から外れるだけ）
3. **YAMLに列挙されたURLのみ** を並列fetch（`sync.parallel` 設定）
4. 新規エントリに自動タグ付与
   - フィード階層から継承されたタグ
   - `feeds.auto_tags` ルールにマッチしたタグ
   - `unread_tag`（常に付与）

### A6.4 DB状態メタデータ（軽量）

```
picofeedr status
# → {"ok": true, "data": {"db_revision": 1284, "last_write_at": 1705420900, "schema_version": 1, "api_version": "0.5.0", "last_sync_at": 1705420800, "last_sync_status": "completed"}, "error": null}
```

**StatusResponse：**

```
{ "db_revision": <int>, "last_write_at": <epoch|null>, "schema_version": <int>, "api_version": "<string>", "last_sync_at": <epoch|null>, "last_sync_status": "completed|partial_failed|failed|null" }
```

### A6.5 一覧検索（軽量メタデータのみ）

```

picofeedr list --query <q> --sort <date_desc|date_asc|first_seen_desc|first_seen_asc> --limit <n> [--cursor <cursor>]

# → {"ok": true, "data": {"total_hits": 342, "items": [EntrySummary...], "next_cursor": "eyJ...", "snapshot_revision": 1284, "snapshot_at": 1705420900}, "error": null}

```

**クエリ例（tag論理式）：**

```
picofeedr list --query 'tag:(A|B)&!C' --sort first_seen_desc --limit 20
picofeedr list --query 'tag:A&B&C -tag:D|E' --sort first_seen_desc --limit 20
```

**sort の意味：**

- `date_*`：`date = COALESCE(published_at, updated_at, first_seen_at)` をキーにソート
- `first_seen_*`：取り込み順（安定・推奨）

**EntrySummary（最小）：**

```

{ "id": 123, "feed_id": 5, "title": "Example Article", "link": "https://example.com/article", "published_at": 1705420800, "first_seen_at": 1705420900, "tags": ["unread", "tech", "rust"] }

```

**ListResponse：**

```
{ "total_hits": <int>, "items": [EntrySummary...], "next_cursor": "<cursor|null>", "snapshot_revision": <int>, "snapshot_at": <epoch|null> }
```

`snapshot_revision` / `snapshot_at` は `status` の `db_revision` / `last_write_at` と同系統の DB メタデータなのだ。  
同一時点の比較・診断用途に使い、ページ継続のキーは常に `next_cursor` を使うのだ。

### A6.6 詳細取得（遅延）

```

picofeedr view <id>

# → {"ok": true, "data": EntryDetail, "error": null}

```

**EntryDetail：**

```

{ "id": 123, "feed_id": 5, "feed_title": "Rust Blog", "title": "Example Article", "link": "https://example.com/article", "author": "John Doe", "published_at": 1705420800, "first_seen_at": 1705420900, "content": "...", "content_type": "text/html", "tags": ["unread", "tech", "rust"], "enclosures": [ {"url": "...", "mime_type": "audio/mpeg", "length": 12345} ] }

```

**content の扱い：**

- `entry_contents.storage` が `none` の場合、本文は存在しないのだ（`content`/`content_type` は `null` または省略されうる）。
- `entry_contents.storage` が `fs` の場合、本文取得に失敗したら `content` は返さない（UIは `link` を外部ブラウザ等で開く方針なのだ）。

**TODO（将来拡張の候補）：**

- `content`/`content_type` を「常にフィールドとして返し、無い場合は `null` に統一する」かどうかを確定するのだ（省略可否を揃える）。
- `content_available: bool` または `content_ref` 等を返して、UI 側の分岐を安定化させるのだ。

### A6.7 状態更新

```

※ 以下の出力例は `--output json` の場合なのだ。plain は人間向けの整形出力になるのだ。

picofeedr mark read   ...

# → {"ok": true, "data": {"updated": 2}, "error": null}

picofeedr mark unread   ...

# → {"ok": true, "data": {"updated": 2}, "error": null}

picofeedr mark tag   ... --add foo,bar --remove baz

# → {"ok": true, "data": {"updated": 2}, "error": null}

picofeedr mark tag   ... --add star

# → {"ok": true, "data": {"updated": 2}, "error": null}

```

### A6.8 タグ

```

picofeedr tags

# → {"ok": true, "data": {"tags": ["unread", "tech", "security", "rust", ...]}, "error": null}

```
