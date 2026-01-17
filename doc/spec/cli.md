# CLI API（JSON出力）

## A6. CLI API（JSON出力）

### A6.1 共通

```
feeder version
# → {"api_version": "0.5.0", "schema_version": 1, "build": "abc123"}

feeder ping
# → {"ok": true}

```

### A6.2 フィード管理

```
feeder feeds
# → {"feeds": [{id, feed_key, url, title, site_url, author, tags}]}

feeder feeds --config-check
# → {"new_in_config": [...], "removed_from_config": [...], "tag_changes": [...]}

```

**注意：**

* フィードの追加/削除は `feeds.yaml` を直接編集
* `sync` 実行時に自動的にDBと同期される
* `feeds` の `tags` は `feeds.yaml` 由来の情報であり、DBの正本ではない（DBは購読の真実を保持しない）

### A6.3 同期（取得）

```
feeder sync
# → {"status": "completed", "fetched": 120, "new_entries": 42, "elapsed": 245.3}
```

**sync の動作フロー：**

1. `feeds.yaml` を読み込み、階層をフラット化（タグ継承・auto_tags をコンパイル）
2. `feeds` カタログを upsert（`feed_key` を算出し、`url`/`title`/`site_url`/`author`/`meta_json` を更新）
   - YAMLから削除されたURL → **何もしない**（履歴保持。同期対象から外れるだけ）
3. **YAMLに列挙されたURLのみ** を並列fetch（`sync.parallel` 設定）
4. 新規エントリに自動タグ付与
   - フィード階層から継承されたタグ
   - `auto_tags` ルールにマッチしたタグ
   - `tags.unread` タグ（常に付与）

### A6.4 一覧検索（軽量メタデータのみ）

```

feeder list --query <q> --sort <date_desc|date_asc|first_seen_desc|first_seen_asc|published_desc|published_asc> --limit <n> [--cursor <cursor>]

# → {"total_hits": 342, "items": [EntrySummary...], "next_cursor": "eyJ..."}

```

**sort の意味：**

- `date_*`：`date = COALESCE(published_at, updated_at, first_seen_at)` をキーにソート
- `first_seen_*`：取り込み順（安定・推奨）
- `published_*`：フィードが主張する公開時刻（欠損がありうる）

**EntrySummary（最小）：**

```

{ "id": 123, "feed_id": 5, "title": "Example Article", "link": "https://example.com/article", "published_at": 1705420800, "first_seen_at": 1705420900, "tags": ["unread", "tech", "rust"] }

```

### A6.5 詳細取得（遅延）

```

feeder view <id>

# → EntryDetail

```

**EntryDetail：**

```

{ "id": 123, "feed_id": 5, "feed_title": "Rust Blog", "title": "Example Article", "link": "https://example.com/article", "author": "John Doe", "published_at": 1705420800, "first_seen_at": 1705420900, "content": "...", "content_type": "text/html", "tags": ["unread", "tech", "rust"], "enclosures": [ {"url": "...", "mime_type": "audio/mpeg", "length": 12345} ] }

```

### A6.6 状態更新

```

feeder mark read   ...

# → {"updated": 2}

feeder mark unread   ...

# → {"updated": 2}

feeder mark star   ...

# → {"updated": 2}

feeder mark unstar   ...

# → {"updated": 2}

feeder mark tag   ... --add foo,bar --remove baz

# → {"updated": 2}

```

### A6.7 タグ

```

feeder tags

# → {"tags": ["unread", "star", "tech", "security", "rust", ...]}

```
