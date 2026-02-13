# API Naming and Schema Rules (v1)

## Purpose

この文書は `--output json` の命名とスキーマ規約を定義するのだ。
未リリース前提のため、v1で曖昧な旧キーは互換なしで廃止するのだ。

## Reserved Envelope Keys

JSONレスポンスのトップレベル予約キーは以下の4つのみなのだ。

- `success`
- `result`
- `error`
- `meta`

## Naming Principles

- 名詞中心で命名する（動詞や状態語の多義利用を避ける）のだ。
- 同じ概念は全エンドポイントで同一キーにするのだ。
- 単位をキー名に含めるのだ。
  - 件数: `*_count`
  - 期間: `*_ms`
  - 時刻: `*_at`（epoch seconds）
- 真偽値は意味語にするのだ（例: `retriable`）。
- ページング継続トークンは `*_token` を使うのだ。

## Type Stability Rules

- 構造を安定させるため、キー省略より `null` を優先するのだ。
- リストは常に配列で返すのだ（空は `[]`）。
- 失敗時は `result = null`、成功時は `error = null` を必須にするのだ。

## Error Rules

`error` オブジェクトは以下を必須にするのだ。

- `code: string`（SCREAMING_SNAKE_CASE）
- `message: string`
- `retriable: bool`
- `details: object|null`

## Meta Rules

`meta` は常に返すのだ。固定キーは以下なのだ。

- `api_version: string`
- `schema_version: integer`
- `generated_at: integer`（epoch seconds）

## Banned / Deprecated Names

v1では次のキーをレスポンス契約で使用しないのだ。

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

- `ok` -> `success`
- `data` -> `result`
- `retry` -> `retriable`
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
