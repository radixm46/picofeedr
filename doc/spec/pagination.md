# ページング仕様（カーソル方式）

## A8. ページング仕様（カーソル方式）

### A8.1 基本

- `OFFSET` は使わず、カーソル（keyset pagination）を基本とする
- `sort` に依存してカーソルを生成する
- `next_cursor` は不透明文字列（内部は `{"k": <sort_key>, "id": <entry_id>}` を JSON→base64url）
- タイブレークは常に `id` を使う（`ORDER BY ..., id ...`）

**TODO（将来拡張の候補）：**

- カーソルに `sort` / `query_hash` / `schema_version` 等を含めて混線（別query/sortの誤用）を検出するのだ。
- 不一致時は `INVALID_CURSOR`（または `INVALID_QUERY`）等で明示的に失敗させるのだ（UIのバグ検知を早める）。

### A8.2 first_seen_desc（推奨：安定）

- 並び順：`ORDER BY first_seen_at DESC, id DESC`
- カーソル内部：`{"k": <first_seen_at>, "id": <entry_id>}`
- 次ページ条件：`WHERE (first_seen_at, id) < (k, id)`

### A8.2b first_seen_asc

- 並び順：`ORDER BY first_seen_at ASC, id ASC`
- カーソル内部：`{"k": <first_seen_at>, "id": <entry_id>}`
- 次ページ条件：`WHERE (first_seen_at, id) > (k, id)`

### A8.3 date_desc（推奨：人間が見る日付）

`published_at` / `updated_at` は欠損しうるので、一覧用の「実効日付」を定義するのだ。

- `date = COALESCE(published_at, updated_at, first_seen_at)`
- 並び順：`ORDER BY date DESC, id DESC`
- カーソル内部：`{"k": <date>, "id": <entry_id>}`
- 次ページ条件：`WHERE (date, id) < (k, id)`

### A8.3b date_asc

- 並び順：`ORDER BY date ASC, id ASC`
- カーソル内部：`{"k": <date>, "id": <entry_id>}`
- 次ページ条件：`WHERE (date, id) > (k, id)`

### A8.4 使用例（first_seen_desc）

```
# 初回
picofeedr list --output json --query unread --sort first_seen_desc --limit 100
# → {"ok": true, "data": {"items": [...], "next_cursor": "eyJrIjoxNzA1NDIwODAwLCJpZCI6MTIzfQ"}, "error": null}

# 2ページ目
picofeedr list --output json --query unread --sort first_seen_desc --limit 100 --cursor "eyJrIjoxNzA1NDIwODAwLCJpZCI6MTIzfQ"
```

### A8.5 使用例（date_desc）

```
# 初回
picofeedr list --query unread --sort date_desc --limit 100

# 2ページ目
picofeedr list --query unread --sort date_desc --limit 100 --cursor "<cursor>"
```
