# Picofeedr 仕様 v0.6

このファイルは仕様書のエントリポイント。本文は `doc/spec/` 配下に分割してある。

## 目次

- `doc/spec/README.md`
- `doc/spec/overview.md`
- `doc/spec/config.md`
- `doc/spec/feeds.md`
- `doc/spec/db.md`
- `doc/spec/api-naming.md`
- `doc/spec/cli.md`
- `doc/spec/query.md`
- `doc/spec/query-date.md`
- `doc/spec/pagination.md`
- `doc/spec/errors.md`
- `doc/spec/workflows.md`

## 方針メモ

- DBスキーマの正本は `doc/db.dbml`、設計意図は `doc/spec/db.md`
- 一覧の「日付」ソートは `date = COALESCE(published_at, updated_at, first_seen_at)` を使う（詳細は `doc/spec/pagination.md`）。
