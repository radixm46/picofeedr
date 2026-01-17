# Feeder 仕様 v0.4

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
* **DBの役割を限定**：SQLiteは「取得結果（entries/tags/content）」と「最小の取得状態（etag/last_modified等）」のみを保持（YAMLの内容・ルール自体は保存しない）

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

### A5.1 方針

* Elfeed系スキーマをベースにタグ中心設計
* **Elfeed互換性**：`id_elfeed` フィールドでElfeedとの相互運用性を確保（オプション）
* **unread/starred は基本タグ**で表現
* 本文（entry content）はストア方式を選べる：`content_store = sqlite | fs | none`
* **JSON形式のmeta**：`feeds.meta` と `entries.meta` はJSON形式で統一

### A5.2 完全なテーブル定義

```
-- ============================================================
-- Meta Table (DB全体のメタ情報)
-- ============================================================
CREATE TABLE es_meta (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

-- スキーマバージョン管理
INSERT INTO es_meta (key, value, updated_at)
VALUES ('schema_version', '1', strftime('%s', 'now'));

-- ============================================================
-- Feeds Table
-- ============================================================
CREATE TABLE feeds (
  id INTEGER PRIMARY KEY,

  -- Elfeed互換性（オプション、Phase 2で実装）
  id_elfeed TEXT UNIQUE,           -- Elfeed互換ID（例：URLの正規化版）

  -- 基本情報
  url TEXT NOT NULL UNIQUE,
  title TEXT,
  site_url TEXT,
  author TEXT,                     -- フィードレベルのauthor
  meta TEXT,                       -- JSON形式（※YAML由来のタグ/ルールは保存しない。キャッシュ用途のみ）

  -- HTTP fetch関連（取得状態）
  etag TEXT,
  last_modified TEXT,              -- RFC 7231形式
  last_fetch_at INTEGER,
  fetch_error TEXT,
  fetch_error_count INTEGER DEFAULT 0,
  -- NOTE: 自動スケジューラ/バックオフを導入する場合は `next_fetch_at` を追加する（Phase拡張）
  -- NOTE: 取得対象の真実は feeds.yaml。DBに enabled を持つ場合は互換/運用都合のキャッシュであり、同期対象の判断には使わない。

  -- タイムスタンプ
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

-- Indexes
CREATE INDEX idx_feeds_id_elfeed ON feeds(id_elfeed);
CREATE INDEX idx_feeds_url ON feeds(url);

-- ============================================================
-- Entries Table
-- ============================================================
CREATE TABLE entries (
  id INTEGER PRIMARY KEY,

  -- Elfeed互換性（オプション、Phase 2で実装）
  id_elfeed TEXT UNIQUE,           -- Elfeed互換ID（例："feed_id\nguid"）

  -- 関連
  feed_id INTEGER NOT NULL,
  guid TEXT NOT NULL,              -- フィード内でユニークなID

  -- 基本情報
  title TEXT NOT NULL,
  link TEXT NOT NULL,
  published_at INTEGER NOT NULL,   -- Unix timestamp (UTC)
  updated_at INTEGER,               -- エントリの更新日時
  author TEXT,

  -- 拡張情報
  meta TEXT,                       -- JSON形式
  content_ref TEXT,                -- content store参照
  content_type TEXT,               -- text/html, text/plain

  -- タイムスタンプ
  created_at INTEGER NOT NULL,     -- DB登録日時

  -- 制約
  FOREIGN KEY(feed_id) REFERENCES feeds(id) ON DELETE CASCADE,
  UNIQUE(feed_id, guid)
);

-- Indexes
CREATE INDEX idx_entries_id_elfeed ON entries(id_elfeed);
CREATE INDEX idx_entries_feed ON entries(feed_id);
CREATE INDEX idx_entries_feed_date ON entries(feed_id, published_at);
CREATE INDEX idx_entries_published ON entries(published_at DESC, id DESC);
CREATE INDEX idx_entries_link ON entries(link);

-- ============================================================
-- Tags Tables (タグ管理)
-- ============================================================
CREATE TABLE tag_ids (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE        -- タグ名（例："unread", "star", "tech"）
);

-- NOTE: tag_ids は辞書テーブル。MVPでは作成時刻は保持しない（必要なら後で追加可能）。

CREATE INDEX idx_tag_ids_name ON tag_ids(name);

CREATE TABLE entry_tags (
  entry_id INTEGER NOT NULL,
  tag_id INTEGER NOT NULL,

  PRIMARY KEY(entry_id, tag_id),
  FOREIGN KEY(entry_id) REFERENCES entries(id) ON DELETE CASCADE,
  FOREIGN KEY(tag_id) REFERENCES tag_ids(id) ON DELETE CASCADE
);

-- Indexes
CREATE INDEX idx_entry_tags_entry ON entry_tags(entry_id);
CREATE INDEX idx_entry_tags_tag ON entry_tags(tag_id);
CREATE INDEX idx_entry_tags_tag_entry ON entry_tags(tag_id, entry_id);

-- ============================================================
-- Content Tables (本文・添付ファイル)
-- ============================================================
CREATE TABLE entry_contents (
  id INTEGER PRIMARY KEY,
  entry_id INTEGER NOT NULL UNIQUE,
  ref TEXT UNIQUE,                 -- 参照キー（fs時のファイル名等）
  content TEXT NOT NULL,
  created_at INTEGER NOT NULL,

  FOREIGN KEY(entry_id) REFERENCES entries(id) ON DELETE CASCADE
);

CREATE INDEX idx_entry_contents_entry ON entry_contents(entry_id);
CREATE INDEX idx_entry_contents_ref ON entry_contents(ref);

CREATE TABLE entry_enclosures (
  id INTEGER PRIMARY KEY,
  entry_id INTEGER NOT NULL,
  url TEXT NOT NULL,
  mime_type TEXT,
  length INTEGER,                  -- バイト数
  created_at INTEGER NOT NULL,

  FOREIGN KEY(entry_id) REFERENCES entries(id) ON DELETE CASCADE,
  UNIQUE(entry_id, url)
);

CREATE INDEX idx_entry_enclosures_entry ON entry_enclosures(entry_id);

-- ============================================================
-- Sync Jobs (削除)
-- ============================================================
-- NOTE: syncの進捗/結果ログはDBに永続化しない。
--       CLI/RPCのレスポンスやstderrの進捗表示で返す（必要ならUI側で保持）。

-- ============================================================
-- FTS5 Table (全文検索、Phase 6)
-- ============================================================
CREATE VIRTUAL TABLE entries_fts USING fts5(
  title,
  content,
  content=entries,
  content_rowid=id
);

-- FTS5自動更新トリガー
CREATE TRIGGER entries_fts_ai AFTER INSERT ON entries BEGIN
  INSERT INTO entries_fts(rowid, title, content)
  VALUES (
    new.id,
    new.title,
    COALESCE((SELECT content FROM entry_contents WHERE entry_id = new.id), '')
  );
END;

CREATE TRIGGER entries_fts_ad AFTER DELETE ON entries BEGIN
  DELETE FROM entries_fts WHERE rowid = old.id;
END;

CREATE TRIGGER entries_fts_au AFTER UPDATE ON entries BEGIN
  UPDATE entries_fts SET title = new.title WHERE rowid = new.id;
END;

CREATE TRIGGER entry_contents_au AFTER UPDATE ON entry_contents BEGIN
  UPDATE entries_fts SET content = new.content WHERE rowid = new.entry_id;
END;

```

### A5.3 本文ストア（content_store）

**解決優先順位（固定）：**

1. `entry_contents` テーブルに該当があればそれを返す
2. なければ `content_ref` を fs として解釈し `data-dir/<content_ref>` を読み込み
3. どちらも無ければ `content=null`

**content_ref の形式：**

* `sqlite` モード：`entry_contents.ref` に保存（NULLも可）
* `fs` モード：SHA256ハッシュ等の安全なファイル名（例：`a1b2c3d4.html`）
* `none` モード：常に NULL

**実装例（Go）：**

```
func (em *EntryManager) GetContent(entryID int64) (string, error) {
    // 1. entry_contents テーブルを確認
    var content string
    err := em.db.QueryRow(`
        SELECT content FROM entry_contents WHERE entry_id = ?
    `, entryID).Scan(&content)

    if err == nil {
        return content, nil
    }

    if err != sql.ErrNoRows {
        return "", err
    }

    // 2. entries.content_ref を確認
    var contentRef sql.NullString
    err = em.db.QueryRow(`
        SELECT content_ref FROM entries WHERE id = ?
    `, entryID).Scan(&contentRef)

    if err != nil {
        return "", err
    }

    if !contentRef.Valid || contentRef.String == "" {
        return "", nil // content なし
    }

    // 3. fs から読み込み
    path := filepath.Join(em.config.Storage.DataDir, contentRef.String)
    data, err := os.ReadFile(path)
    if err != nil {
        return "", err
    }

    return string(data), nil
}

```

### A5.4 Elfeed互換性（オプション、Phase 2）

**IDの生成方法：**

```go
// feeds.id_elfeed の生成
func GenerateFeedIDElfeed(url string) string {
    // Phase 1: URLをそのまま使う（シンプル）
    return url

    // Phase 2: Elfeed完全互換（要elfeed-db.el解析）
    // return elfeedCanonicalID(url)
}

// entries.id_elfeed の生成
func GenerateEntryIDElfeed(feedIDElfeed, guid string) string {
    // Elfeed形式: "feed-id\nentry-guid"
    return fmt.Sprintf("%s\n%s", feedIDElfeed, guid)
}
```

Phase 1では  id_elfeed をNULLのままでOK（Elfeed移行が不要なら）

### A5.5 JSON meta の活用

**feeds.meta の例：**

```
{
  "subtitle": "...",
  "language": "en",
  "generator": "...",
  "custom": {"key": "value"}
}

```

**SQLite JSON1拡張での検索（Phase 6）：**

```
-- tagsにrustを含むフィード
SELECT * FROM feeds
WHERE json_extract(meta, '$.tags') LIKE '%rust%';

-- カスタムフィールドで検索
SELECT * FROM feeds
WHERE json_extract(meta, '$.custom_field') = 'value';

```

**Go実装例：**

```
type FeedMeta struct {
    Tags           []string               `json:"tags"`
    UpdateInterval int                    `json:"update_interval,omitempty"`
    Custom         map[string]interface{} `json:",inline"`
}

func (f *Feed) GetMeta() (*FeedMeta, error) {
    if f.Meta == "" {
        return &FeedMeta{Tags: []string{}}, nil
    }

    var meta FeedMeta
    if err := json.Unmarshal([]byte(f.Meta), &meta); err != nil {
        return nil, err
    }
    return &meta, nil
}

func (f *Feed) SetMeta(meta *FeedMeta) error {
    data, err := json.Marshal(meta)
    if err != nil {
        return err
    }
    f.Meta = string(data)
    return nil
}

```

## A6. CLI API（JSON出力）

### A6.1 共通

```
feeder version
# → {"api_version": "0.3.0", "schema_version": 1, "build": "abc123"}

feeder ping
# → {"ok": true}

```

### A6.2 フィード管理

```
feeder feeds
# → {"feeds": [{id, url, title, enabled, tags}]}

feeder feeds --config-check
# → {"new_in_config": [...], "removed_from_config": [...], "tag_changes": [...]}

```

**注意：**

* フィードの追加/削除は `feeds.yaml` を直接編集
* `sync` 実行時に自動的にDBと同期される

### A6.3 同期（取得）

```
feeder sync
# → {"status": "completed", "fetched": 120, "new_entries": 42, "elapsed": 245.3}
```

**sync の動作フロー：**

1. `feeds.yaml` を読み込み、階層をフラット化（タグ継承・auto_tags をコンパイル）
2. DB内のfeeds状態（etag/last_modified等）を照合
   - 新規URL → INSERT（状態行を作る。取得対象の真実はあくまでYAML）
   - YAMLから削除されたURL → **何もしない**（履歴保持。同期対象から外れるだけ）
3. **YAMLに列挙されたURLのみ** を並列fetch（`sync.parallel` 設定）し、etag/last_modified/last_fetch_at/error等を更新
4. 新規エントリに自動タグ付与
   - フィード階層から継承されたタグ
   - `auto_tags` ルールにマッチしたタグ
   - `tags.unread` タグ（常に付与）

### A6.4 一覧検索（軽量メタデータのみ）

```

feeder list --query  --sort <published_desc|published_asc> --limit  [--cursor ]

# → {"total_hits": 342, "items": [EntrySummary...], "next_cursor": "eyJ..."}

```

**EntrySummary（最小）：**

```

{ "id": 123, "published_at": 1705420800, "feed_id": 5, "title": "Example Article", "tags": ["unread", "tech", "rust"], "link": "[https://example.com/article](https://example.com/article)" }

```

### A6.5 詳細取得（遅延）

```

feeder view

# → EntryDetail

```

**EntryDetail：**

```

{ "id": 123, "feed_id": 5, "feed_title": "Rust Blog", "title": "Example Article", "link": "[https://example.com/article](https://example.com/article)", "published_at": 1705420800, "author": "John Doe", "content": "...", "content_type": "text/html", "tags": ["unread", "tech", "rust"], "enclosures": [ {"url": "...", "mime_type": "audio/mpeg", "length": 12345} ] }

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

-- tag:security EXISTS ( SELECT 1 FROM entry_tags et JOIN tag_ids t ON et.tag_id = t.id WHERE et.entry_id = entries.id AND t.name = 'security' )

-- -tag:misc NOT EXISTS ( SELECT 1 FROM entry_tags et JOIN tag_ids t ON et.tag_id = t.id WHERE et.entry_id = entries.id AND t.name = 'misc' )

```

## A8. ページング仕様（カーソル方式）

### A8.1 基本

- `OFFSET` は使わず、カーソル（keyset pagination）を基本とする
- `sort` に依存してカーソルを生成
- `next_cursor` は不透明文字列（内部は `published_at,id` 等の順序キー）

### A8.2 published\_desc（推奨）

- 並び順：`ORDER BY published_at DESC, id DESC`
- カーソル内部：`{"k": <published_at>, "id": <entry_id>}` を JSON→base64url
- 次ページ条件：`WHERE (published_at, id) < (k, id)`

### A8.3 使用例

```

# 初回

feeder list --query unread --sort published_desc --limit 100

# → {"items": [...], "next_cursor": "eyJrIjoxNzA1NDIwODAwLCJpZCI6MTIzfQ"}

# 2ページ目

feeder list --query unread --sort published_desc --limit 100
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

6. `tag_ids` / `entry_tags` テーブル操作
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

sqlite3 ~/.local/share/feeder/db.sqlite <<EOF DELETE FROM entries WHERE id IN ( SELECT e.id FROM entries e WHERE e.published_at < strftime('%s', 'now', '-30 days') AND NOT EXISTS ( SELECT 1 FROM entry_tags et JOIN tag_ids t ON et.tag_id = t.id WHERE et.entry_id = e.id AND t.name IN ('unread', 'star') ) ); EOF

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

## v0.4（2026-01-17）

- **SQLiteスキーマを統合版に更新**
  - Elfeed互換性：`id_elfeed` フィールド追加（feeds/entries、オプション）
  - インデックス最適化：複合インデックス `(feed_id, published_at)` 等を追加
  - `feeds.author` フィールド追加
  - `entry_contents.ref` フィールド追加（fs/sqlite統一的な扱い）
  - `entry_enclosures.length` を INTEGER に変更
  - `entries.date` → `entries.published_at` に変更（明確化）
  - `tag_ids.txt` → `tag_ids.name` に変更
  - `es_meta` テーブル追加（`config` テーブルから改名）
  - FTS5テーブルとトリガーの追加（Phase 6）
  - タイムスタンプフィールドの統一
- **JSON meta の明確化**
  - `feeds.meta` と `entries.meta` はJSON形式で統一
  - SQLite JSON1拡張での検索例を追加
- **content\_store 実装の詳細化**
  - 解決優先順位の実装例（Go）を追加
  - `entry_contents.ref` の活用方法を明記

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
