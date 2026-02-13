# エラー仕様（CLI）

## A10. エラー仕様（CLI）

### A10.1 失敗時のJSON（`--output json`）

```json
{ "status": "error", "result": null, "error": { "code": "DB_LOCKED", "message": "database is locked", "retryable": true, "details": { "sqlite_code": "DatabaseBusy", "retry_after_ms": 200 } }, "meta": { "api_version": "<string>", "db_schema_version": <int>, "generated_at": <epoch> } }
```

`status="error"` は致命失敗を示すのだ。`result` は必ず `null`、`error` は必須なのだ。

### A10.2 `error` 共通フィールド

- `code: string`（SCREAMING_SNAKE_CASE）
- `message: string`
- `retryable: bool`
- `details: object|null`

### A10.3 `error.details` の段階導入（現行）

現行実装では以下の `error.code` で機械可読な `details` を返すのだ。対象外コードは `details=null` を許容するのだ。

- `INVALID_QUERY`
  - shape: `{ "kind": "<reason_kind>", "field": "<field|null>", "value": <any|null>, "hint": "<hint|null>" }`
  - 例: `--limit 0` -> `{ "kind": "limit_out_of_range", "field": "limit", "value": 0, "hint": "limit_must_be_greater_than_zero" }`
  - 例: 不正cursor -> `{ "kind": "invalid_cursor", "field": "cursor", "value": "not-a-cursor", "hint": "base64url_decode_failed" }`

- `ENTRY_NOT_FOUND`
  - shape: `{ "resource": "entry", "entry_id": <int> }`

- `CONFIG_ERROR`
  - shape: `{ "path": "<path|null>", "hint": "<hint|null>" }`

- `DB_LOCKED`
  - shape: `{ "sqlite_code": "<DatabaseBusy|DatabaseLocked|null>", "retry_after_ms": 200 }`

### A10.4 exit code

- 致命（fatal）は **exit code != 0** + 上記 JSON なのだ。
- `sync` の fetch/parse 失敗は致命ではなく、exit code は 0 のまま `result.errors` に積むのだ。
- `feeds --config-check` は validation report を `status="ok"` または `status="warning"` で返し、`result.valid=false` のときは `status="warning"` かつ exit code 1 を返すのだ。
- `stdout` への書き込みで `BrokenPipe` が起きた場合は、下流の早期終了として扱い、exit code 0 で終了するのだ。

### A10.5 sync errors（非致命）

`picofeedr sync` の `result.errors` 配列で使うコード例なのだ。

- `FETCH_FAILED`
- `PARSE_FAILED`
