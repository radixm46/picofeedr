# エラー仕様（CLI）

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

### A10.3 sync errors（非致命）

`feeder sync` の `errors` 配列で使うコード例なのだ（exit code は 0 のまま継続するのだ）。

- `FETCH_FAILED` - フィード取得失敗（ネットワーク/HTTPなど）
- `PARSE_FAILED` - フィードパース失敗（不正XML/Atom/RSS）
