# 変更履歴

## v0.8（2026-02-14, breaking）

- **list/view/mark の公開IDを opaque string へ移行**
  - `list.items[].id` を `entry_id` へ変更
  - `list.items[].feed_id` を integer から opaque string へ変更
  - `view` / `mark` は `entry_id` 指定へ変更
- **list レスポンスを正規化**
  - `result.feeds` を追加し、`feed_id` と `title` の対応表を返す
  - `items[].feed_id` で `feeds[]` を参照する契約へ変更

## v0.7（2026-02-13, breaking）

- **CLI JSON envelope v1 へ全面刷新**
  - `ok/data/error` を `status/result/error/meta` に変更
  - `status`（`ok|warning|error`）へ一本化
  - `meta` に `api_version`/`db_schema_version`/`generated_at` を追加
- **エラーpayloadを明確化**
  - `retry` を `retryable` に変更
  - `details` フィールドを追加（nullable）
- **同期レスポンスの単位を明示**
  - `fetched` -> `fetched_feed_count`
  - `failed` -> `failed_feed_count`
  - `new_entries` -> `new_entry_count`
  - `elapsed` -> `duration_ms`
- **一覧/状態更新レスポンスを命名統一**
  - `total_hits` -> `total_count`
  - `next_cursor` -> `next_page_token`
  - `updated` -> `updated_entry_count`
  - `updated_at`/`sync_at`/`sync_status` -> `last_write_at`/`last_sync_at`/`last_sync_status`
- **DBスキーマ仕様追随: effective_date ソート向け式インデックスを明記**
  - `entries` に `COALESCE(published_at, updated_at, first_seen_at)` ベースの索引を追加
  - 追加索引: `(..., id)` / `(feed_id, ..., id)`

## v0.6（2026-02-09）

- **仕様追随: `list` カーソル内部仕様とページング例を現行実装へ同期**
  - `next_cursor` の内部仕様を `k/id/sort/query_hash`（JSON→base64url）として明記
  - カーソル不一致時の失敗コードを `INVALID_QUERY` に統一して明文化
  - `pagination` のJSON例を `ListResponse`（`total_hits`, `revision`, `updated_at` を含む）に合わせて更新
- **Breaking: `feeds --config-check` を差分表示から静的妥当性検証へ変更**
  - 出力を `{valid, errors, warnings, checked_feeds}` に変更
  - `new_in_config` / `removed_from_config` / `tag_changes` を廃止
  - `valid=false` のときのみ exit code 1 に変更
- **`feeds.meta_json` への tags 保存を廃止**
  - `feeds.yaml` が唯一の真実である方針を明文化

## v0.5（2026-01-17）

 - **DB設計の正本を `doc/db.dbml` / `doc/spec/db.md` に寄せる**
  - `id_elfeed` 前提の記述を仕様から除去
  - テーブル/カラム名を `meta_json`/`tags`/`first_seen_at` 等に統一
  - 本文ストアを `entry_contents` 中心の規約に再整理
- **ページングの推奨ソートを `first_seen_desc` に変更**
  - `published_at` 欠損を許容する設計に合わせ、安定基準を明確化

## v0.4（2026-01-17）

- （この版の内容は v0.5 で設計変更のため一部撤回）
- **SQLiteスキーマを統合版に更新**
  - インデックス最適化：複合インデックス `(feed_id, published_at)` 等を追加
  - `feeds.author` フィールド追加
  - `entry_contents.ref` フィールド追加（fs/sqlite統一的な扱い）
  - `entry_enclosures.length` を INTEGER に変更
  - `entries.date` → `entries.published_at` に変更（明確化）
  - `es_meta` テーブル追加（`config` テーブルから改名）
  - FTS5テーブルとトリガーの追加（Phase 5）
  - タイムスタンプフィールドの統一
- **JSON meta の明確化**
  - （削除済み）SQLite JSON1拡張での検索例を追加

## v0.3（2026-01-16）

- 設定ファイルを2層構造に分離（`config.toml` + `feeds.yaml`）
- フィード管理を設定ファイル駆動に変更（CLIでのCRUD廃止）
- タグ中心設計の明確化（unread含む）
- feeds.yamlの階層構造とタグ継承を追加
- 自動タグルールを feeds.yaml に統合
- CLI/RPC両対応を明記
- 実装順序の詳細化（Phase 0-7）
- ユーザーワークフローの追加

## v0.2（2026-01-16初期）

- Backend/CLI仕様の初期ドラフト
- Elfeed系スキーマ採用
- カーソルベースページング
- 都度CLI実行を基本とする方針

## v0.1（構想）

- 基本コンセプトの策定
