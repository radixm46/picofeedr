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

