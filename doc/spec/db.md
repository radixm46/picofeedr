# SQLite / データモデル（規約）

## A4. 並行性・ロック・整合性

このプロジェクトでは DB スキーマを一度固めたら頻繁には動かさない前提なのだ。スキーマ変更が必要になった場合は、`doc/db.dbml` と `doc/db-note.md` を同時に更新し、マイグレーション方針も別途定義するのだ。

* クライアント:バックエンドは原則 1:1（フロントは常にCLI/RPC経由）
* DBは単一writer（バックエンドのみ書く）
* SQLiteはWAL前提

### A4.1 sync と閲覧/更新

* `sync` はネットワークI/Oを含むため、DBロック時間を短くする

  * 推奨：feed単位（または小バッチ）で `BEGIN...COMMIT`
* `entries.list/get` と `entries.mark` は短トランザクションで即時反映

### A4.2 SQLite 推奨設定（実装メモ）

* `busy_timeout` を設定（例：5000ms）
* `journal_mode=WAL`
* `synchronous=NORMAL`
* 定期的な `PRAGMA wal_checkpoint(TRUNCATE)` 実行（sync完了後等）

## A5. SQLite データモデル

### A5.1 方針（正本とする設計）

* SQLiteのテーブル定義の正本は `db.dbml` とする（この仕様書は規約・運用上の意味を補う）
* **購読（subscription）の真実**は外部設定（`feeds.yaml` 等）であり、DBは「索引＋状態（タグ）＋帰属（provenance）」を保持する
* **状態はタグ**で表現する（`unread`/`star` など）
* 拡張フィールドは **`*_meta_json`（JSON text）** に逃がす（SQLite JSON1前提）
* **時刻は3種類**：
  - `published_at`：ソースが主張する公開時刻（欠損/嘘を許容）
  - `updated_at`：ソースが主張する更新時刻（欠損/嘘を許容）
  - `first_seen_at`：ローカルが初めて観測した時刻（安定基準、NOT NULL）

### A5.2 基本テーブル（`db.dbml` の要点）

#### `es_meta`

* 単一行テーブル（`id=1` をアプリ側で保証）
* `meta_json` は JSON object（例：`schema_version`、作成日時、マイグレーション履歴等）

#### `feeds`

* フィードの「購読管理」ではなく **帰属/表示のためのカタログ**
* `feed_key`（UNIQUE, NOT NULL）：アプリ定義の安定ID（例：正規化URL、ハッシュ）
* `url`（NOT NULL）：取得/表示の基準URL
* `title`/`author`/`site_url`/`meta_json` は任意
* 取得状態（ETag/Last-Modified等）を永続化したい場合は **`feeds.meta_json` の予約キー**に格納する（例：`{"http":{"etag":"...","last_modified":"...","last_fetch_at":1700000000}}`）

#### `entries`

* エントリ索引の中核
* `entry_key`（UNIQUE, NOT NULL）：アプリ定義の安定ID
* `feed_id`（NOT NULL）：`feeds.id` 参照（削除は RESTRICT）
* `source_id`：Atom `<id>` / RSS `<guid>` / フォールバック（nullable）
* `link`/`title`：欠損を許容（nullable）
* `published_at`/`updated_at`：欠損を許容（nullable）
* `first_seen_at`（NOT NULL）：安定したタイムライン用
* `meta_json`：カテゴリ、rawフィールド等（JSON）

#### `tags` / `entry_tags`

* `tags`：タグ辞書（`name` UNIQUE）
* `entry_tags`：中間（`PRIMARY KEY(entry_id, tag_id)`）
* `entries` 削除時：`entry_tags` は CASCADE

#### `entry_contents`

* 1:1 本文（`entry_id` がPKかつ `entries.id` へのFK）
* `storage`（NOT NULL）：`none`/`db`/`fs`（値の妥当性はアプリ側で保証）
* `ref`：`storage='fs'` のときの鍵（例：sha256 hex）。パスそのものは持たない（nullable）
* `content_type`：`text/html`/`text/plain` 等（nullable）
* `content`：`storage='db'` の場合に本文を格納（nullable）

#### `entry_enclosures`

* 添付（`UNIQUE(entry_id, url)`）
* `length` は INTEGER（バイト数、nullable）

### A5.3 本文ストア（`entry_contents`）

**解決優先順位（固定）：**

1. `entry_contents` 行が無ければ `content=null`
2. `storage='none'` → `content=null`
3. `storage='db'` → `entry_contents.content` を返す（`content_type` も同様）
4. `storage='fs'` → `entry_contents.ref`（hash key）から保存パスを導出し、ファイルシステムから取得（導出ルール/ルートはCLI設定）

### A5.4 JSON meta の活用（例）

**`feeds.meta_json` の例：**

```
{
  "subtitle": "...",
  "language": "en",
  "generator": "...",
  "http": {"etag": "...", "last_modified": "...", "last_fetch_at": 1700000000}
}
```

**SQLite JSON1拡張での検索例：**

```
-- http.etag が一致する feed を探す
SELECT * FROM feeds
WHERE json_extract(meta_json, '$.http.etag') = '...';
```
