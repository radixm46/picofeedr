# API Naming and Schema Rules (v1)

## Purpose

この文書は `--output json` の命名とスキーマ規約を定義する。
未リリース前提のため、v1で曖昧な旧キーは互換なしで廃止する。

## Reserved Envelope Keys

JSONレスポンスのトップレベル予約キーは以下の4つのみ。

- `status`
- `result`
- `error`
- `meta`

`status` は応答判定の単一軸。

- `ok`: 正常終了
- `warning`: 非致命の注意付き成功
- `error`: 致命失敗

## Naming Principles

- 名詞中心で命名する（動詞や状態語の多義利用を避ける）。
- 同じ概念は全エンドポイントで同一キーにする。
- 単位をキー名に含める。
  - 件数: `*_count`
  - 期間: `*_ms`
  - 時刻: `*_at`（epoch seconds）
- 真偽値は意味語にするのだ（例: `retryable`）。
- ページング継続トークンは `*_token` を使う。
- 参照IDはリソース名を接頭辞にした `*_id` を使うのだ（例: `entry_id`, `feed_id`）。
- 公開IDは opaque string として扱い、値の内部構造を前提にしない。
- 内部DB主キーは `*_pk` として命名し、外部契約へ露出しない。

## Type Stability Rules

- 構造を安定させるため、キー省略より `null` を優先する。
- リストは常に配列で返すのだ（空は `[]`）。
- `result` と `error` は常に返すのだ（欠損させない）。
- `status="error"` では `result = null` かつ `error != null` を必須にする。
- `status in {"ok","warning"}` では `result != null` かつ `error = null` を必須にする。

## Error Rules

`error` オブジェクトは以下を必須にする。

- `code: string`（SCREAMING_SNAKE_CASE）
- `message: string`
- `retryable: bool`
- `details: object|null`

`details` は段階導入とし、`error.code` ごとに shape を拡張していく。  
現時点の厳密shapeは `doc/spec/errors.md` を正本とする。

## Meta Rules

`meta` は常に返す。固定キーは以下のとおり。

- `api_version: string`
- `db_schema_version: integer`
- `generated_at: integer`（epoch seconds）

## Banned / Deprecated Names

v1では次のキーをレスポンス契約で使用しない。

- `ok`
- `data`
- `retry`
- `fetched`
- `failed`
- `new_entries`
- `elapsed`
- `updated`
- `total_hits`
- `next_cursor`
- `updated_at`（status/listメタ用途として）
- `sync_at`
- `sync_status`

## Canonical Renames (v1)

- `success` + `severity` -> `status`
- `data` -> `result`
- `retry` -> `retryable`
- `fetched` -> `fetched_feed_count`
- `failed` -> `failed_feed_count`
- `new_entries` -> `new_entry_count`
- `elapsed` -> `duration_ms`
- `updated` -> `updated_entry_count`
- `total_hits` -> `total_count`
- `next_cursor` -> `next_page_token`
- `updated_at` -> `last_write_at`
- `sync_at` -> `last_sync_at`
- `sync_status` -> `last_sync_status`
