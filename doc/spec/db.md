# DB 設計（正本への参照）

このドキュメントは DB 設計の「要約」なのだ。正本は以下なのだ。

- スキーマ（テーブル/カラム/制約/参照/インデックス）：`doc/db.dbml`
- 設計方針・運用想定：`doc/db-note.md`

## 規約（他ドキュメントが前提にすること）

- 購読（subscription）の真実は外部設定で、DBは「索引＋状態（タグ）＋帰属」を保持するのだ。
- タグは状態として扱い、`tags`/`entry_tags` が割当の正になるのだ。
- 時刻は `published_at` / `updated_at` / `first_seen_at` を使い、`published_at`/`updated_at` は欠損を許容するのだ。
- 本文は `entry_contents.storage` が `db|fs|none` で、`fs` の `ref` は hash key（パスは持たない）のだ。
