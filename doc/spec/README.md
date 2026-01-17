# Feeder Spec (Split Docs)

このディレクトリは `doc/spec.md` を分割した仕様書群なのだ。

## Source of Truth

- DBスキーマ（テーブル/カラム/制約/参照/インデックス）：`doc/db.dbml`
- DB設計意図・運用想定：`doc/spec/db.md`

`doc/spec/*.md` は上記の正本と矛盾しない範囲で、CLI規約や運用上の不変条件を説明するのだ。

## Document Map

- `doc/spec/overview.md`：ゴール、実行形態（CLI/RPC）
- `doc/spec/config.md`：`config.toml` / `feeds.yaml`（自動タグ含む）
- `doc/spec/db.md`：SQLiteデータモデル（要点・規約）
- `doc/spec/cli.md`：CLIコマンドとJSON入出力
- `doc/spec/query.md`：検索クエリ言語
- `doc/spec/pagination.md`：カーソルページング仕様
- `doc/spec/errors.md`：エラー仕様
- `doc/spec/roadmap.md`：実装フェーズ（MVP順）
- `doc/spec/ui-notes.md`：UI/クライアント設計ノート（非規約）
- `doc/spec/workflows.md`：ユーザーワークフロー
- `doc/spec/impl-guide.md`：実装ガイド
- `doc/spec/references.md`：比較・参考リンク
- `doc/spec/changelog.md`：変更履歴

## Notes

- FTS5（全文検索）は日本語トークナイズ等の検討が必要なので後回し（仕様ではPhase拡張として扱う）のだ。
