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

