# Feeder 仕様 v0.6

このファイルは仕様書のエントリポイントなのだ。本文は `doc/spec/` 配下に分割してあるのだ。

## 目次

- `doc/spec/README.md`
- `doc/spec/overview.md`
- `doc/spec/config.md`
- `doc/spec/db.md`
- `doc/spec/cli.md`
- `doc/spec/query.md`
- `doc/spec/pagination.md`
- `doc/spec/errors.md`
- `doc/spec/roadmap.md`
- `doc/spec/ui-notes.md`（非規約）
- `doc/spec/workflows.md`
- `doc/spec/impl-guide.md`
- `doc/spec/tech-selection.md`
- `doc/spec/references.md`
- `doc/spec/changelog.md`

## 方針メモ

- DBスキーマの正本は `doc/db.dbml`、設計意図は `doc/spec/db.md` なのだ。
- 一覧の「日付」ソートは `date = COALESCE(published_at, updated_at, first_seen_at)` を使うのだ（詳細は `doc/spec/pagination.md`）。
