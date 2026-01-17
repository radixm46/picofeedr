# Feeder 仕様 v0.5

## ドキュメント分割（整理）計画

`spec.md` が肥大してきたので、内容を以下に分割して見通しを良くする（作業計画）：

- `spec/overview.md`：ゴール/非ゴール、実行形態（CLI/RPC）
- `spec/config.md`：`config.toml` / `feeds.yaml`（自動タグ含む）
- `spec/db.md`：SQLiteデータモデル（`db.dbml` を正として要点・規約を記載）
- `spec/cli.md`：CLIコマンドとJSON入出力
- `spec/query.md`：検索クエリ言語とSQL生成の考え方
- `spec/pagination.md`：カーソルページング仕様
- `spec/errors.md`：エラー仕様
- `spec/roadmap.md`：実装フェーズ（MVP順）
- `spec/ui-notes.md`：UI/クライアント設計ノート（非規約）
- `spec/workflows.md`：ユーザーワークフロー
- `spec/impl-guide.md`：実装ガイド（言語/依存/テスト）
- `spec/references.md`：比較・参考リンク

移行手順（最小の安全策）：

1. まず `spec.md` の本文をそれぞれの新ファイルへ移動し、`spec.md` は目次＋リンク＋変更履歴のみにする
2. 既存の見出しアンカーに依存している箇所がある場合は、旧アンカーのリンクを `spec.md` 側に残す（リダイレクト的に）
3. 移行後に `rg` で旧テーブル名（`tag_ids` など）や旧仕様（Elfeed互換）を検索し、取りこぼしを潰す

このドキュメントは **Backend/CLI（規約）** と **UI/クライアント（設計ノート）** を分離して記述する。

* **Backend/CLI**：実装が守るべき仕様（データモデル、コマンド、入出力、整合性）
* **UI/クライアント**：推奨するUI構成やキャッシュなど（規約ではない）

---

# A. Backend / CLI 仕様（規約）

## A0. ゴール

* RSS/Atom取得・正規化・検索・状態更新・永続化（SQLite）を担当
* **ローカル完結**：ネットワークは `sync` のみ。閲覧/状態更新はローカルSQLiteのみ
* **シンプル優先**：都度CLI起動（1コマンド=1操作）で成立することを最優先
* **依存最小**：DB=SQLite。実装言語はGo/Rust想定（外部サービスや常駐前提を置かない）
* **UI差し替え可能**：Emacs/TUI/GUI等、任意のフロントがCLIを叩けば同じ機能を使える
* **タグ中心設計**：unread/starred含むすべての状態をタグで管理
* **設定ファイル駆動**：取得対象・タグ継承・自動タグルールは **feeds.yaml が唯一の真実**
* **DBの役割を限定**：SQLiteは「取得結果（entries/tags/content）」と、必要なら「条件付きGETのキャッシュ（ETag/Last-Modified等）」を `feeds.meta_json` に保持する（YAMLの内容・ルール自体は保存しない）

## A1. 目的 / 非目的

### 目的

* RSS/Atomの取得（ETag/Last-Modified対応）
* 正規化データをSQLiteへ保存（単一writer）
* 検索・ソート・ページングをバックエンド側で完結
* タグベースの状態管理：unread/starred/カスタムタグ
* 設定ファイル（YAML）からのフィード管理
* CLI Mode（都度実行）を基本とする（RPCは将来拡張として扱う）

### 非目的（当面）

* フィードのCRUD操作（設定ファイルを直接編集）
* 古いエントリの自動削除（SQLiteを直接操作）
* 多クライアントの厳密同期
* リモート公開前提の認証・権限管理

## A2. 実行形態

### A2.1 CLI Mode（デフォルト）

* `feeder <command>` を都度起動し、標準出力でJSONを返す
* 成功：exit code 0 + JSON
* 失敗：exit code !=0 + JSON（機械可読エラー）

**コマンド一覧：**

```
feeder sync                      # 同期実行
feeder list [--query <q>]        # エントリ一覧
feeder view <id>                 # エントリ詳細
feeder mark <operation> <ids>    # 状態更新
feeder tags                      # タグ一覧
feeder feeds                     # フィード一覧
feeder feeds --config-check      # 設定ファイルとDB差分表示

```

**共通フラグ：**

```
--config <path>     # config.toml のパス（デフォルト: ~/.config/feeder/config.toml）
--db <path>         # DB パスの上書き（テスト用）

```

### A2.2 RPC Mode（オプション）

* `feeder serve` でstdio JSON-RPC 2.0サーバー起動
* 双方向通信、進捗通知（notification）対応

**メソッド一覧：**

* `sync.run` → 進捗通知: `sync.progress`
* `entries.list`
* `entries.get`
* `entries.mark`
* `feeds.list`
* `tags.list`

**Notification例：**

* `sync.progress(current, total, feed)`
* `entries.updated(entry_ids)`
* `log(level, message)`

## A3. 設定ファイル（2層構造）

### A3.1 config.toml（CLI動作設定）

アプリケーションの**動作方法**を定義（変更頻度：低）

```
# ~/.config/feeder/config.toml

[database]
path = "~/.local/share/feeder/db.sqlite"

[sync]
parallel = 5              # 並列fetch数
timeout = 30              # HTTP timeout（秒）
user_agent = "feeder/0.1.0"
retry_count = 3
retry_delay = 5

[storage]
content_store = "sqlite"  # sqlite | fs | none
data_dir = "~/.local/share/feeder/data"

[tags]
unread = "unread"         # 未読タグ名
starred = "star"          # スタータグ名

[query]
default_limit = 100
max_limit = 1000

[feeds]
source = "~/.config/feeder/feeds.yaml"

[log]
level = "info"
file = "~/.local/share/feeder/feeder.log"

```

### A3.2 feeds.yaml（フィード一覧・自動タグ）

**データの内容**を定義（変更頻度：高）

```
# ~/.config/feeder/feeds.yaml

feeds:
  tech:
    tags: [tech]
    programming:
      tags: [programming]
      rust:
        tags: [rust]
        feeds:
          - url: https://blog.rust-lang.org/feed.xml
            title: Rust Blog
          - url: https://this-week-in-rust.org/rss.xml
      go:
        tags: [golang]
        feeds:
          - url: https://go.dev/blog/feed.atom
    security:
      tags: [security, important]
      feeds:
        - url: https://security.googleblog.com/feeds/posts/default
        - url: https://krebsonsecurity.com/feed/

  news:
    tags: [news]
    feeds:
      - url: https://news.ycombinator.com/rss
        title: Hacker News
      - url: https://lobste.rs/rss

auto_tags:
  - title_regex: '(?i)CVE-\d{4}-\d+'
    add_tags: [cve, security-alert]
    priority: 10
  - title_contains: [vulnerability, exploit, 0-day]
    add_tags: [security-alert]
    priority: 20

```

**階層構造とタグ継承：**

* 親グループのタグは子グループに継承される
* 例：`tech.programming.rust` のフィードは `[tech, programming, rust]` タグを持つ

## A4. 並行性・ロック・整合性

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
* **時刻は2種類**：
  - `published_at`：ソースが主張する時刻（欠損/嘘を許容）
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
* `storage`（NOT NULL）：`none`/`db`/`fs`/`obj` 等（値の妥当性はアプリ側で保証）
* `ref`：ファイルパス/オブジェクトキー等（nullable）
* `content_type`：`text/html`/`text/plain` 等（nullable）
* `content`：`storage='db'` の場合に本文を格納（nullable）
* `content_hash`：sha256等（任意）

#### `entry_enclosures`

* 添付（`UNIQUE(entry_id, url)`）
* `length` は INTEGER（バイト数、nullable）

### A5.3 本文ストア（`entry_contents`）

**解決優先順位（固定）：**

1. `entry_contents` 行が無ければ `content=null`
2. `storage='none'` → `content=null`
3. `storage='db'` → `entry_contents.content` を返す（`content_type` も同様）
4. `storage!='db'` → `entry_contents.ref` を解釈して外部ストアから取得（実装依存）

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
   - 条件付きGET（ETag/Last-Modified等）を使う場合は `feeds.meta_json` にキャッシュする（例：`http.etag`）
4. 新規エントリに自動タグ付与
   - フィード階層から継承されたタグ
   - `auto_tags` ルールにマッチしたタグ
   - `tags.unread` タグ（常に付与）

### A6.4 一覧検索（軽量メタデータのみ）

```

feeder list --query <q> --sort <first_seen_desc|first_seen_asc|published_desc|published_asc> --limit <n> [--cursor <cursor>]

# → {"total_hits": 342, "items": [EntrySummary...], "next_cursor": "eyJ..."}

```

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

## A7. 検索クエリ言語

### A7.1 Phase 4（タグフィルタのみ、MVP）

**サポート構文：**

- `unread` - `tag:unread` のショートカット
- `star` または `starred` - `tag:star` のショートカット
- `tag:security` - 指定タグを持つエントリ
- `-tag:misc` - 指定タグを持たないエントリ
- スペース区切りは AND 条件

**例：**

```

feeder list --query "unread tag:security -tag:misc"

# → 未読 AND securityタグあり AND miscタグなし

```

### A7.2 Phase 6（拡張クエリ）

**追加構文：**

- `text:"keyword"` - 全文検索（FTS5）
- `feed:123` または `feed:"Feed Title"` - 特定フィード
- `before:2026-01-01` / `after:2025-12-01` - 日付範囲

**例：**

```

feeder list --query 'unread text:"rust" after:2026-01-01'

```

### A7.3 SQL生成

**タグフィルタの例：**

```

-- tag:security EXISTS ( SELECT 1 FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE et.entry_id = entries.id AND t.name = 'security' )

-- -tag:misc NOT EXISTS ( SELECT 1 FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE et.entry_id = entries.id AND t.name = 'misc' )

```

## A8. ページング仕様（カーソル方式）

### A8.1 基本

- `OFFSET` は使わず、カーソル（keyset pagination）を基本とする
- `sort` に依存してカーソルを生成
- `next_cursor` は不透明文字列（内部は `first_seen_at,id` 等の順序キー）

### A8.2 first_seen\_desc（推奨）

- 並び順：`ORDER BY first_seen_at DESC, id DESC`
- カーソル内部：`{"k": <first_seen_at>, "id": <entry_id>}` を JSON→base64url
- 次ページ条件：`WHERE (first_seen_at, id) < (k, id)`

### A8.3 使用例

```

# 初回

feeder list --query unread --sort first_seen_desc --limit 100

# → {"items": [...], "next_cursor": "eyJrIjoxNzA1NDIwODAwLCJpZCI6MTIzfQ"}

# 2ページ目

feeder list --query unread --sort first_seen_desc --limit 100
--cursor "eyJrIjoxNzA1NDIwODAwLCJpZCI6MTIzfQ"

```

## A9. 自動タグ（feeds.yaml）

### A9.1 ルール定義

```

auto_tags:

* feed_url: "[https://example.com/feed.xml](https://example.com/feed.xml)" add_tags: [favorite] priority: 5

* title_contains: [Rust, Go, Python] add_tags: [programming] priority: 10

* title_regex: '(?i)CVE-\d{4}-\d+' add_tags: [cve, security-alert] priority: 20

```

### A9.2 適用タイミング

- 新規エントリの取り込み時のみ（`sync` 実行時）
- ルール変更を過去分に遡及しない

### A9.3 タグ付与順序

1. フィード階層から継承されたタグ
2. `auto_tags` ルール（優先度順）
3. `tags.unread` タグ（常に最後）

## A10. エラー仕様（CLI）

### A10.1 失敗時のJSON

```

{ "error": { "code": "DB_LOCKED", "message": "Database is locked, please retry", "retry": true } }

```

### A10.2 error code 例

- `FEED_NOT_FOUND` - 指定されたfeed\_idが存在しない
- `ENTRY_NOT_FOUND` - 指定されたentry\_idが存在しない
- `INVALID_QUERY` - クエリ構文エラー
- `DB_LOCKED` - データベースロック（リトライ推奨）
- `SYNC_IN_PROGRESS` - 既に同期中
- `CONFIG_ERROR` - 設定ファイルエラー

## A11. MVP 実装順（推奨）

### Phase 0：プロジェクト基盤（Week 1前半）

1. プロジェクト構成
2. `config.toml` 読み込み
3. `feeds.yaml` 読み込み（階層パース、タグ継承）
4. Database初期化・マイグレーション
5. 基本的なCLI構造

### Phase 1：タグシステム（Week 1後半）

6. `tags` / `entry_tags` テーブル操作
7. TagManager実装
8. CLI: `feeder tags`
9. テスト

### Phase 2：フィード管理（Week 2前半）

10. `feeds` テーブル操作
11. `feeds.yaml` とDBの同期（reconcileFeeds）
12. CLI: `feeder feeds`
13. CLI: `feeder feeds --config-check`

### Phase 3：同期処理（Week 2後半）

14. RSS/Atom fetch実装（ETag/Last-Modified対応）
15. Entry正規化・保存
16. 自動タグルール適用
17. 並列fetch実装（worker pool）
18. CLI: `feeder sync`

**マイルストーン1：基本同期完成**

### Phase 4：エントリ一覧・タグフィルタ（Week 3）

19. Query parser（タグフィルタのみ）
20. entries.query 実装（カーソルページング）
21. CLI: `feeder list`
22. CLI: `feeder view`
23. CLI: `feeder mark`

**マイルストーン2：実用可能（ここまでで120 feedsでも快適）**

### Phase 5：クエリ拡張（Week 4前半〜後半）

28. FTS5テーブル作成・更新
29. クエリ言語拡張（text:, feed:, before:/after:）
30. CLI: 拡張クエリ対応
31. テスト

**マイルストーン3：高度な検索完成**

### Phase 6：JSON-RPC Mode（Week 5-6）

32. JSON-RPC server実装（stdio）
33. `sync.run` with notifications
34. 他のメソッド実装
35. Emacsクライアント（RPC版）
36. モード切り替え機能

**マイルストーン4：RPC Mode完成**

---

# B. UI / クライアント設計ノート（非規約）

## B0. 役割分担（クライアント側）

- 画面：一覧表示、フィルタ入力、選択、本文表示
- 操作：既読/スター/タグ付与・除去
- ページング：`next_cursor` の保持と次ページ要求
- UI都合の状態（選択中のID、表示順、ローカル索引）はクライアントが保持

## B1. クライアントキャッシュ（推奨）

### B1.1 feed index キャッシュ

- `feeds` コマンドの結果をローカル保存
- 次回起動時に差分確認（変更があれば再取得）

### B1.2 tag index キャッシュ（任意）

- `tags` コマンドの結果を同様に保存

## B2. ページングのUI運用（推奨）

- `total_hits` は変動しうるため参考値として扱う
- `sort` 変更時は cursor を破棄して再検索
- 大量ヒット時でもUIが重くならないよう、既定 `limit=100` を守り、追加ロードで増やす

## B3. "読んだら消える"運用の想定

- `unread` 一覧を読み進めて既読化する場合、 続き取得は「最後に受け取った `next_cursor`」を使う
- クライアントは表示上 `unread` を外したアイテムを即時反映してよい

## B4. バックエンドモードの選択

### B4.1 CLI Mode（推奨デフォルト）

**使用ケース：**

- 120 feeds以下
- 手動sync中心
- シンプルな実装を優先

**性能：**

- プロセス起動オーバーヘッド：約10ms/回（Go/Rust）
- syncはユーザーが明示的に叩く前提（常時ポーリング前提にしない）

### B4.2 RPC Mode（パワーユーザー向け）

**使用ケース：**

- 真のリアルタイム進捗が欲しい
- entry単位の通知が必要
- 複雑なワークフロー

**利点：**

- プロセス起動なし
- 即座の進捗通知
- 双方向通信

## B5. Emacsクライアント実装例（CLI Mode）

```

(defun feeder-sync () "Run sync synchronously and refresh list." (interactive) (let ((result (feeder-cli-json "sync"))) (message "Sync completed: fetched=%s new_entries=%s" (alist-get 'fetched result) (alist-get 'new_entries result)) (feeder-refresh-list)))

(defun feeder-cli-json (&rest args) "Run feeder command and parse JSON output." (with-temp-buffer (apply #'call-process "feeder" nil t nil args) (goto-char (point-min)) (json-parse-buffer :object-type 'alist)))

(defun feeder-list () "Show entry list." (interactive) (let ((result (feeder-cli-json "list" "--query" "unread" "--limit" "100"))) ;; Display items... ))

(defun feeder-mark-read (&rest ids) "Mark entries as read." (apply #'feeder-cli-json "mark" "read" (mapcar #'number-to-string ids)))

```

---

# C. ユーザーワークフロー

## C1. 初期セットアップ

```

# 1. 設定ファイル作成

mkdir -p ~/.config/feeder cp /usr/share/feeder/config.example.toml ~/.config/feeder/config.toml cp /usr/share/feeder/feeds.example.yaml ~/.config/feeder/feeds.yaml

# 2. feeds.yaml 編集

vim ~/.config/feeder/feeds.yaml

# 3. 初回同期

feeder sync

# 4. エントリ確認

feeder list --query unread

```

## C2. 日常利用

```

# 朝：同期

feeder sync

# Emacsで閲覧

emacs -f feeder

# または CLI で確認

feeder list --query "unread tag:security" | jq '.items[] | {id, title}' feeder view 123 feeder mark read 123

```

## C3. フィード追加

```

# 1. feeds.yaml を編集

vim ~/.config/feeder/feeds.yaml

# 新規追加:

# feeds:

# tech:

# programming:

# rust:

# feeds:

# - url: [https://new-blog.example.com/feed.xml](https://new-blog.example.com/feed.xml)

# tags: [new]

# 2. 差分確認

feeder feeds --config-check

# 3. 同期（自動的に新規フィードが追加される）

feeder sync

```

## C4. 古いエントリの削除（直接SQL）

```

# 30日以上前の既読エントリを削除

sqlite3 ~/.local/share/feeder/db.sqlite <<EOF DELETE FROM entries WHERE id IN ( SELECT e.id FROM entries e WHERE e.published_at < strftime('%s', 'now', '-30 days') AND NOT EXISTS ( SELECT 1 FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE et.entry_id = e.id AND t.name IN ('unread', 'star') ) ); EOF

```

---

# D. 実装ガイド

## D1. 推奨言語：Go

**理由：**

- 起動速度：10ms未満（CLIに最適）
- クロスコンパイル：簡単（Linux/Mac/Win）
- JSON-RPC：`github.com/sourcegraph/jsonrpc2`（実績あり）
- SQLite：`github.com/mattn/go-sqlite3`（安定）
- RSS/Atom：`github.com/mmcdole/gofeed`（実績あり）
- YAML：`gopkg.in/yaml.v3`（標準的）

## D2. プロジェクト構成（Go）

```

feeder/ ├── cmd/ │   └── feeder/ │       └── main.go ├── internal/ │   ├── config/ │   │   ├── config.go          # config.toml │   │   └── feeds.go           # feeds.yaml │   ├── database/ │   │   ├── db.go │   │   ├── migration.go │   │   └── models.go │   ├── tag/ │   │   └── manager.go │   ├── feed/ │   │   └── fetcher.go │   ├── entry/ │   │   ├── manager.go │   │   └── query.go │   ├── sync/ │   │   └── syncer.go │   └── rpc/ │       └── server.go ├── config.example.toml ├── feeds.example.yaml ├── go.mod └── README.md

```

## D3. 依存ライブラリ（Go）

```go
// go.mod
module github.com/yourusername/feeder

go 1.21

require (
    github.com/BurntSushi/toml v1.3.2
    github.com/mattn/go-sqlite3 v1.14.18
    github.com/mmcdole/gofeed v1.2.1
    github.com/urfave/cli/v2 v2.27.0
    gopkg.in/yaml.v3 v3.0.1

    // RPC Mode用（Phase 6）
    // github.com/sourcegraph/jsonrpc2 v0.2.0
)
```

## D4. テスト戦略

```

# ユニットテスト

go test ./...

# 統合テスト（テスト用DB）

feeder --config test.toml --db :memory: sync

# E2Eテスト

./test/e2e.sh

```

---

# E. 参考情報

## E1. Elfeedとの比較

| 機能 Elfeed Feeder |              |               |
| ---------------- | ------------ | ------------- |
| プラットフォーム         | Emacs専用      | UI非依存         |
| 設定ファイル           | Emacs Lisp   | YAML          |
| タグ管理             | タグベース        | タグベース（同じ）     |
| 自動タグ             | elfeed-org   | feeds.yaml    |
| 検索               | Emacs buffer | SQLite + FTS5 |
| 同期               | elisp        | Go/Rust（高速）   |
| 並列化              | 限定的          | 設定可能          |

## E2. Himalayaとの比較

| 機能 Himalaya Feeder |           |               |
| ------------------ | --------- | ------------- |
| 対象                 | Email     | RSS/Atom      |
| CLI設計              | 都度実行      | 都度実行 + RPC    |
| 出力形式               | JSON      | JSON          |
| UI                 | TUI/Emacs | TUI/Emacs（予定） |
| 設定                 | TOML      | TOML + YAML   |

## E3. 参考リンク

- Elfeed: [https://github.com/skeeto/elfeed](https://github.com/skeeto/elfeed)
- elfeed-org: [https://github.com/remyhonig/elfeed-org](https://github.com/remyhonig/elfeed-org)
- Himalaya: [https://github.com/soywod/himalaya](https://github.com/soywod/himalaya)
- gofeed: [https://github.com/mmcdole/gofeed](https://github.com/mmcdole/gofeed)
- JSON-RPC 2.0: [https://www.jsonrpc.org/specification](https://www.jsonrpc.org/specification)

---

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
