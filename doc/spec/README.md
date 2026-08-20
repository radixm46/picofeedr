# Picofeedr Spec (Split Docs)

このディレクトリは `doc/spec.md` を分割した仕様書群。

## Source of Truth

- DBスキーマ（テーブル/カラム/制約/参照/インデックス）：`doc/db.dbml`
- DB設計意図・運用想定：`doc/spec/db.md`

`doc/spec/*.md` は上記の正本と矛盾しない範囲で、CLI規約や運用上の不変条件を説明する。

## Keep Policy

- 残すのは「外部契約」「運用上の不変条件」「コードだけでは読みにくい設計理由」。
- コード構成案、技術選定の叩き台、将来構想の工程表はここに置かない。
- 将来案を書く場合は、現行契約と混ざらないように Draft と明記する。

## Document Map

- `doc/spec/overview.md`：ゴール、CLI の実行形態
- `doc/spec/config.md`：`config.toml` / `feeds.yaml`（自動タグ含む）
- `doc/spec/feeds.md`：`feeds.yaml` の読み込み仕様（現行実装準拠）
- `doc/spec/feeds.sample.yaml`：架空URLで書いた `feeds.yaml` サンプル
- `doc/spec/db.md`：DB設計方針と運用想定
- `doc/spec/api-naming.md`：JSON命名規約と型安定ルール
- `doc/spec/cli.md`：CLIコマンドとJSON入出力
- `doc/spec/query.md`：検索クエリ言語
- `doc/spec/query-date.md`：日付検索の現行仕様
- `doc/spec/pagination.md`：カーソルページング仕様
- `doc/spec/errors.md`：エラー仕様
- `doc/spec/workflows.md`：ユーザーワークフロー
- `schemas/`：`schemas` バイナリで自動生成するコマンド別JSON Schema成果物

## Notes

- FTS5（全文検索）は日本語トークナイズ等の検討が必要なので後回しとする（仕様ではPhase拡張として扱う）。
- JSON Schema は `cargo run --bin schemas` で再生成し、差分をコミットする。
