# ページング仕様（カーソル方式）

## A8. ページング仕様（カーソル方式）

### A8.1 基本

- `OFFSET` は使わず、カーソル（keyset pagination）を基本とする
- `sort` に依存してカーソルを生成する
- `next_page_token` は不透明文字列（内部は `{\"k\": <sort_key>, \"id\": <entry_id>, \"sort\": \"<sort>\", \"query_hash\": \"<sha1-hex>\"}` を JSON→base64url）
- タイブレークは常に `id` を使う（`ORDER BY ..., id ...`）
- トークンの `sort` または `query_hash` が現在の要求と不一致なら `INVALID_QUERY` で失敗させる
- クライアントは `next_page_token` の内部JSONを解釈せず、受け取った値をそのまま次ページ要求へ再送する

### A8.2 first_seen_desc（推奨：安定）

- 並び順：`ORDER BY first_seen_at DESC, id DESC`
- トークン内部：`{"k": <first_seen_at>, "id": <entry_id>, "sort": "first_seen_desc", "query_hash": "<sha1-hex>"}`
- 次ページ条件：`WHERE (first_seen_at, id) < (k, id)`

### A8.3 date_desc（推奨：人間が見る日付）

`published_at` / `updated_at` は欠損しうるので、一覧用の「実効日付」を定義するのだ。

- `date = COALESCE(published_at, updated_at, first_seen_at)`
- 並び順：`ORDER BY date DESC, id DESC`
- トークン内部：`{"k": <date>, "id": <entry_id>, "sort": "date_desc", "query_hash": "<sha1-hex>"}`
- 次ページ条件：`WHERE (date, id) < (k, id)`

### A8.4 使用例（first_seen_desc）

```bash
# 初回
picofeedr --output json list --query unread --sort first_seen_desc --limit 100
# → {"status": "ok", "result": {"total_count": 342, "items": [...], "next_page_token": "eyJ...", "revision": 1284, "last_write_at": 1705420900}, "error": null, "meta": {...}}

# 2ページ目
picofeedr --output json list --query unread --sort first_seen_desc --limit 100 --cursor "eyJ..."
```
