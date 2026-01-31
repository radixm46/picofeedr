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
feeder version
# → {"ok": true, "data": {"api_version": "0.5.0", "schema_version": 1, "build": "abc123"}, "error": null}

feeder ping
# → {"ok": true, "data": {"ok": true}, "error": null}

```

### A6.2 フィード管理

```
feeder feeds
# → {"ok": true, "data": {"feeds": [{id, feed_key, url, title, site_url, author, tags}]}, "error": null}

feeder feeds --config-check
# → {"ok": true, "data": {"new_in_config": [...], "removed_from_config": [...], "tag_changes": [...]}, "error": null}

```

**注意：**

* フィードの追加/削除は `feeds.yaml` を直接編集
* `sync` 実行時に自動的にDBと同期される
* `feeds` の `tags` は `feeds.yaml` 由来の情報であり、DBの正本ではない（DBは購読の真実を保持しない）

### A6.3 同期（取得）

```
feeder sync
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

1. `feeds.yaml` を読み込み、階層をフラット化（タグ継承・auto_tags をコンパイル）
2. `feeds` カタログを upsert（`feed_key` を算出し、`url`/`title`/`site_url`/`author`/`meta_json` を更新）
   - YAMLから削除されたURL → **何もしない**（履歴保持。同期対象から外れるだけ）
3. **YAMLに列挙されたURLのみ** を並列fetch（`sync.parallel` 設定）
4. 新規エントリに自動タグ付与
   - フィード階層から継承されたタグ
   - `auto_tags` ルールにマッチしたタグ
   - `unread_tag`（常に付与）

### A6.4 一覧検索（軽量メタデータのみ）

```

feeder list --query <q> --sort <date_desc|date_asc|first_seen_desc|first_seen_asc> --limit <n> [--cursor <cursor>]

# → {"ok": true, "data": {"total_hits": 342, "items": [EntrySummary...], "next_cursor": "eyJ..."}, "error": null}

```

**sort の意味：**

- `date_*`：`date = COALESCE(published_at, updated_at, first_seen_at)` をキーにソート
- `first_seen_*`：取り込み順（安定・推奨）

**EntrySummary（最小）：**

```

{ "id": 123, "feed_id": 5, "title": "Example Article", "link": "https://example.com/article", "published_at": 1705420800, "first_seen_at": 1705420900, "tags": ["unread", "tech", "rust"] }

```

### A6.5 詳細取得（遅延）

```

feeder view <id>

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

### A6.6 状態更新

```

feeder mark read   ...

# → {"ok": true, "data": {"updated": 2}, "error": null}

feeder mark unread   ...

# → {"ok": true, "data": {"updated": 2}, "error": null}

feeder mark tag   ... --add foo,bar --remove baz

# → {"ok": true, "data": {"updated": 2}, "error": null}

feeder mark tag   ... --add star

# → {"ok": true, "data": {"updated": 2}, "error": null}

```

### A6.7 タグ

```

feeder tags

# → {"ok": true, "data": {"tags": ["unread", "tech", "security", "rust", ...]}, "error": null}

```
