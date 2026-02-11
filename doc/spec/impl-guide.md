# D. 実装ガイド

この章は「Rustで実装する」前提のガイドなのだ。技術選定の背景は `doc/spec/tech-selection.md` を参照するのだ。

## D1. 推奨言語：Rust

**理由：**

- 起動が速く、単一バイナリで配布しやすい（CLIに最適）なのだ。
- 型と所有権でバグを潰しやすく、長期運用で効くのだ。
- SQLite/HTTP/パース/CLI周りの実績あるクレートが揃っているのだ。
- 将来RPC Modeを追加しても、境界設計（Repository/DB actor）で拡張しやすいのだ。

## D2. プロジェクト構成（Rust・ドラフト）

```
picofeedr/
├── Cargo.toml
├── migrations/            # SQL migrations（採用する場合）
├── src/
│   ├── main.rs            # CLI entry
│   ├── cli.rs             # clap definitions / dispatch
│   ├── config/
│   │   ├── mod.rs         # config.toml
│   │   └── feeds.rs       # feeds.yaml
│   ├── db/
│   │   ├── mod.rs         # Store trait + types
│   │   ├── migrate.rs     # migrations runner
│   │   └── sqlite.rs      # rusqlite adapter
│   ├── feed/
│   │   ├── mod.rs         # fetch + parse + normalize
│   │   ├── atom_rss.rs
│   │   └── jsonfeed.rs
│   ├── entry/
│   │   ├── mod.rs         # domain model + ops
│   │   └── query.rs
│   └── sync/
│       └── mod.rs         # sync orchestration
└── tests/
    └── cli.rs             # CLI integration tests
```

## D3. 依存ライブラリ（Rust・ドラフト）

厳密なバージョン固定は実装開始時に決めるのだ（ここでは「何を使うか」だけを示すのだ）。

### 必須（MVP）

- CLI：`clap`（derive）
- エラー：`anyhow`（アプリ境界）、`thiserror`（ドメイン/インフラ）
- 直列化：`serde` + `serde_json`
- 設定：`toml`（config.toml）、`serde_yaml_ng`（feeds.yaml）
- DB（同期）：`rusqlite`
- RSS/Atom：`feed-rs`
- JSON Feed：`jsonfeed`（またはserdeで自前）

### テスト（推奨）

- CLI統合：`assert_cmd` + `predicates`
- 一時ファイル：`tempfile`
- パラメタライズ：`rstest`（必要になったら）
- スナップショット：`insta`（必要になったら）

### HTTP（候補）

- 同期実装：`ureq`
- 非同期（将来）：`reqwest` + `tokio`（RPC Modeを本格化させる段階で採用検討するのだ）

## D4. テスト戦略（ドラフト）

```
# ユニットテスト
cargo test

# CLI統合テスト（assert_cmd）
cargo test --test cli
```

---
