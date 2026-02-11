# エラー仕様（CLI）

## A10. エラー仕様（CLI）

### A10.1 失敗時のJSON（`--output json`）

```

{ "ok": false, "data": null, "error": { "code": "DB_LOCKED", "message": "Database is locked, please retry", "retry": true } }

```

**exit code：**

- 致命（fatal）は **exit code != 0** + 上記 JSON なのだ。
- `sync` の fetch/parse 失敗は致命ではなく、exit code は 0 のまま `data.errors` に積むのだ（A10.3）。
- `feeds --config-check` は validation report を `ok=true` で返し、`data.valid=false` のときのみ exit code 1 を返すのだ。

**TODO（将来拡張の候補）：**

- 自動化（cron/CI）向けに、`sync status=failed`（全件失敗）のときだけ exit code を警告扱いで変えるモードを検討するのだ（例: `--strict` 等）。stdout の JSON 契約は維持するのだ。

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

`picofeedr sync` の `errors` 配列で使うコード例なのだ（exit code は 0 のまま継続するのだ）。

- `FETCH_FAILED` - フィード取得失敗（ネットワーク/HTTPなど）
- `PARSE_FAILED` - フィードパース失敗（不正XML/Atom/RSS）

### A10.4 config-check validation issue codes（`data.errors` / `data.warnings`）

- `DUPLICATE_FEED_URL` - 同一URLが複数feedとして定義されている
- `EMPTY_FEED_URL` - feedのurlが空文字
- `INVALID_AUTO_TAG_RULE` - `auto_tags` の定義不備（例：`add_tags` 空、条件未指定）
- `DUPLICATE_FEED_TAG` - 同一feed定義で同じtagが重複（warning）
