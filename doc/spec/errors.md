# エラー仕様（CLI）

## A10. エラー仕様（CLI）

### A10.1 失敗時のJSON（`--output json`）

```

{ "ok": false, "data": null, "error": { "code": "DB_LOCKED", "message": "Database is locked, please retry", "retry": true } }

```

**exit code：**

- 致命（fatal）は **exit code != 0** + 上記 JSON なのだ。
- `sync` の fetch/parse 失敗は致命ではなく、exit code は 0 のまま `data.errors` に積むのだ（A10.3）。

### A10.2 error code 例

- `FEED_NOT_FOUND` - 指定されたfeed\_idが存在しない
- `ENTRY_NOT_FOUND` - 指定されたentry\_idが存在しない
- `INVALID_QUERY` - クエリ構文エラー
- `DB_LOCKED` - データベースロック（リトライ推奨）
- `SYNC_IN_PROGRESS` - 既に同期中
- `CONFIG_ERROR` - 設定ファイルエラー
- `DB_ERROR` - DBエラー（リトライ不要の想定）
- `IO_ERROR` - I/Oエラー
- `SERIALIZATION_ERROR` - シリアライズ/パース系のエラー
- `INTERNAL` - 想定外の致命（panic含む）。詳細は stderr（debug/trace時）に出してよいのだ

### A10.3 sync errors（非致命）

`feeder sync` の `errors` 配列で使うコード例なのだ（exit code は 0 のまま継続するのだ）。

- `FETCH_FAILED` - フィード取得失敗（ネットワーク/HTTPなど）
- `PARSE_FAILED` - フィードパース失敗（不正XML/Atom/RSS）
