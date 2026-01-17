# 変更履歴

## v0.5（2026-01-17）

- **DB設計の正本を `db.dbml` / `db-note.md` に寄せる**
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
  - FTS5テーブルとトリガーの追加（Phase 6）
  - タイムスタンプフィールドの統一
- **JSON meta の明確化**
  - SQLite JSON1拡張での検索例を追加

## v0.3（2026-01-16）

- 設定ファイルを2層構造に分離（`config.toml` + `feeds.yaml`）
- フィード管理を設定ファイル駆動に変更（CLIでのCRUD廃止）
- タグ中心設計の明確化（unread/starred含む）
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
