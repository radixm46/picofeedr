# DB 設計方針と運用想定

このドキュメントは DB 設計（方針・運用想定）の正本なのだ。

スキーマ（テーブル/カラム/制約/参照/インデックス）の正本は `doc/db.dbml` なのだ。

> 重要：本DBは「購読（subscription）の真実」を保持しない。購読は外部設定（YAML/TOML等）が真実で、DBは **エントリ索引＋状態（タグ）＋帰属（provenance）** を保持する。

---

## 1. 設計のゴール

* **高速に検索できる索引DB**（entries を中心としたインデックス）
* **帰属提示のための最小限の feed カタログ**（feeds は購読管理ではない）
* **状態管理（unread 等）をタグで表現**し、手動で頻繁に付け外しできる
* 本文は **ハッシュベースのファイルツリー**に保存する運用を主軸にしつつ、必要ならDB内保存も可能（スイッチ）
* スキーマは SQLite + JSON1 を前提とし、拡張は **meta_json** に逃がす

---

## 2. 非目標（やらないこと）

* **購読リストの保持・同期**（source-of-truth は外部設定）
* **タグのルール再評価エンジン**（ルール変更で全件再ラベル付け、など）

  * 代わりに UI/CLI で「検索→対象集合→一括タグ追加/削除」を提供
* RSS/Atom の原構造を完全に正規化して永続化（Atom link 多重などを厳密再現しない）
* 取得ジョブログ（fetch runs）をDBに保持する

---

## 3. コンポーネントの責務分担

### 3.1 外部設定（YAML/TOML 等）

* 購読対象の定義（URL、取得頻度、初期タグなど）
* DBや本文ストアの再構築時の入力

### 3.2 SQLite DB（本設計）

* entries の索引（検索・一覧表示・並べ替え）
* feeds の帰属情報（entry がどの feed に属するかの提示）
* tags/entry_tags による状態管理（unread など）

### 3.3 本文ストア（推奨：ファイルツリー）

* content-addressed（例：sha256）でファイル分割し、クラウド同期時の差分処理を軽くする
* 本文をDB外に置くことで、DBファイルの肥大化を抑えやすい（※DB破損時の完全復元性は前提にしない）

### 3.4 SQLite実装レイヤの責務境界

* `schema/` は DDL と migration 資産のみを持つ
* `query/` は SQL 定数と SQL ビルダを持つ（実行しない、業務ロジックを持たない）
* DAO（`entries.rs` / `feeds.rs` / `tags.rs` / `meta.rs`）は単一文SQLの実行のみを担当する
* DAO は `pub(crate)` を維持し、`db::sqlite` の内部実装として扱う
* `repo/` は複数DAOをまたぐユースケースの調停を担当する
  * 読み取り経路は `*ReadRepo`
  * 書き込み経路は `*WriteRepo`（`Tx` 配下で実行）
* 新しい書き込みフローは `SqliteStore::tx()` + `*WriteRepo` を使う
* `SqliteStore::transaction()` は互換レイヤ用途に限定し、新規実装では使わない

---

## 4. テーブルの役割（概要）

### 4.1 `es_meta`

* 単一行の JSON メタ
* `schema_version`、作成日時、アプリID、将来のマイグレーション履歴など

### 4.2 `feeds`

* **購読ではなく帰属**のためのカタログ
* `feed_id` はアプリ定義の安定ID
* 現行実装では `feed_id = "k_" + base64url_nopad(sha256(feed_url_bytes))`
  * 例: `k_nEYNGhY1VhMY6HOx32gKp764cXqV8XUpAdM2Js3GBQA`
* 旧形式（URL文字列そのもの等）の `feed_id` は移行用の一時状態であり、最終DBには残さないのだ
* `url/title/site_url/meta_json` などは表示・説明のための最小情報
* `meta_json` には `feeds.yaml` の設定値（tags / auto_tags ルール）を保存しないのだ

### 4.3 `entries`

* 本DBの中核：エントリ索引
* `id`：DB内部参照用の整数PK（JOIN最適化、外部キーのサイズ削減）
* `entry_id`: Stable app-defined ID (unique)
  * `entry_id = "k_" + base64url_nopad(sha256("{feed_id}:{source_id}"))`
* `source_id`: Canonical identifier string in the form `{namespace}|{cleaned_id}`
  * `namespace` uses `feed_id`
  * `cleaned_id` is selected in order:
    1. feed-provided id/guid
    2. link
    3. `urn:sha1:<sha1(content)>`
    4. `urn:sha1:<sha1(title|published_at|updated_at|author)>`
    5. last resort `urn:sha1:<sha1(seed)>`（※seedは決定的に構成する。値が全て空でも同一seedになるため、衝突しうるのだ）
  * `cleaned_id` trims and collapses whitespace
* `published_at` / `updated_at`：ソースが主張する時刻（欠損・嘘を許容）
* `first_seen_at`：ローカルが初めて観測した時刻（NOT NULL）

  * ソートや新着判定の **安定基準**として使用
* `meta_json`：カテゴリ、複数author、rawフィールド等の拡張の逃がし先

### 4.4 `entry_contents`

* 1:1 の本文管理（**DB内保存** or **ハッシュFS参照**）

* `storage` は **3値**：
  * `db`：SQLite に本文を格納
  * `fs`：sha256 hex（小文字64桁）を鍵に、保存パスをアプリ側で導出して参照
    * 例：`ref=b5bb...` → `./data/b5/b5bb...`（`./data` は CLI 設定）
  * `none`：このエントリには **本文が無い**（エントリに content/summary が無い等）

* カラム設計の意図（最小・明快）：
  * `ref`：`storage='fs'` のときの **鍵（=hash hex）**（※パスそのものは入れない）
  * `content`：`storage='db'` のときの本文
  * `content_type`：`text/html` / `text/plain` など（任意：レンダリング用）

> 注：`fs` の置き場所（rootディレクトリ）やパス導出ルールはレコードに持たず、CLI本体設定で与える。

> 注：`storage` と `ref/content` の整合はアプリ側または CHECK 制約で担保する（例：`fs` なら `ref` 必須・`content` は空、`db` なら逆）。

### 4.5 `tags` / `entry_tags`

* タグは **自由命名（Unicode）**、階層や命名規則の強制はしない
* `tags.name` が表示名であり、別名/内部名の分離は当面しない
* `entry_tags` は junction のみ（`origin` は不要：再評価エンジンを想定しないため）

---

## 5. 運用想定

### 5.0 IDエンコードの互換契約

* `feed_id` / `entry_id` のエンコードは `k_ + base64url_nopad(sha256(...))` で固定する
* 互換性のため、IDのバイト列入力は UTF-8 とする
* base64 は URL-safe alphabet（`A-Z a-z 0-9 - _`）を使い、`=` パディングは付けない
* `k_` 接頭辞は「ハッシュ由来のopaque ID」であることを示す識別子として予約する
* 公開IDは opaque として扱う契約を維持する（クライアントは分解・再生成を前提にしない）

### 5.1 未読管理（タグで実施）

* CLI 本体設定に `unread_tag` を持つ（デフォルト `unread`）
* 新規取得した entry には必ず `unread_tag` を付与
* 既読化は `unread_tag` の削除
* 再取得・更新時に、既存エントリの未読状態（unreadタグ）をルール的に上書き・再付与しない

### 5.2 タグ検索・合成（検索syntaxで解決）

* DBはタグ階層を持たず、検索syntaxでグルーピングする
* 例：`group/tag1 group/tag2` を OR 合成して一覧取得
* 例：`group/*` を `tags.name LIKE 'group/%'` として展開
* AND/NOT などは将来拡張（例：`+tag` `-tag`）

### 5.3 時刻の扱い

* `published_at`/`updated_at` 欠損を許容する（RSSでは pubDate が任意、壊れたフィードも多い）
* 安定した並び替えや新着判定は `first_seen_at` を併用
* 「公開日が無いので published_at に受信時刻を代入する」方式は、意味の混線を起こしやすいので採用しない

### 5.4 同期・バックアップ

* 想定：SQLite の単一DBファイルをクラウドストレージ側で同期・世代管理してもらう（アプリ側で特別な復元機構を前提にしない）
* 本文をファイルツリーに置くことで、DBファイルの肥大化を抑えやすい（クラウド同期の負荷も下げやすい）
* DBは索引・状態（タグ）を含むため、破損・巻き戻りで過去エントリや未読状態が失われる可能性は受け入れる
* 同期の安定性のため、運用上は WAL/同期モードに注意（例：WAL の `-wal/-shm` を同時に同期できるかを確認）

### 5.5 移行後DBの期待不変条件

他システムから移行した後、`sync` 実行前に次を満たしていることを期待するのだ。

* `feeds.feed_id` は全件 `k_` 形式である（旧形式IDを残さない）
* 同一 `feeds.url` に対して `feeds` 行は高々1件
* `entries.entry_id` は全件 `k_` 形式である
* 同一実体を表す重複エントリ（同一 `feed_url + link` の多重行）を残さない

上記を満たさない場合、既存実体が新規として再計上され、`first_seen_at` 基準の一覧で偏りが発生しうるのだ。

---

## 6. 設計上の判断メモ

* `source_id` は保持：移行・デバッグ・外部照合のため（内部PK `id` と役割が別）
* `source_id_type` は不要：必要なら `source_id` を自己記述にして型情報を内包
* `last_seen_at` は削除：RSS/Atom が rolling window（最新N件）であることが多く、単純な消失検知に向かないため
* タグはキャッシュではなく状態：unread 等の状態管理に使うため、ルール再評価で全再生成する思想を取らない

---

## 7. 今後の拡張余地（必要になったら）

* タグの階層：DBではなく検索syntaxで対応する方針。必要なら `tags.parent_id` 等を追加
* 複数リンク（Atom link 多重）：厳密保存が必要になった時に `entry_links` テーブル追加
* 高速全文検索：SQLite FTS5 を追加し、本文は ref 参照でもインデックス可能な設計へ

---

## 8. 参考：設計の核（短文）

* **購読は外部設定が真実**
* **DBは索引＋状態（タグ）＋帰属**
* **本文はハッシュFSが主役**
* **タグは自由、階層は検索syntaxで表現**
