# エラー仕様

## Scope

この文書は `--output json` における致命エラー契約と、`sync` の非致命エラー契約を定義する。

## Fatal Error Envelope

致命失敗時の JSON は次の shape を持つ。

```json
{ "status": "error", "result": null, "error": { "code": "DB_LOCKED", "message": "database is locked", "retryable": true, "details": { "sqlite_code": "DatabaseBusy", "retry_after_ms": 200 } }, "meta": { "api_version": "<string>", "db_schema_version": <int>, "generated_at": <epoch> } }
```

### Rules

- `status = "error"` は致命失敗を示す
- `result` は必ず `null`
- `error` は必須
- 致命失敗は exit code != 0

## Error Object

`error` オブジェクトの必須フィールドは次の4つ。

- `code: string`
- `message: string`
- `retryable: bool`
- `details: object|null`

## Structured `details`

現行実装では次の `error.code` で機械可読な `details` shape を持つ。  
それ以外のコードでは `details = null` を許容する。

### `INVALID_QUERY`

```json
{ "kind": "<reason_kind>", "field": "<field|null>", "value": <any|null>, "hint": "<hint|null>" }
```

例:

```json
{ "kind": "limit_out_of_range", "field": "limit", "value": 0, "hint": "limit_must_be_greater_than_zero" }
```

```json
{ "kind": "invalid_cursor", "field": "cursor", "value": "not-a-cursor", "hint": "base64url_decode_failed" }
```

### `ENTRY_NOT_FOUND`

```json
{ "resource": "entry", "entry_id": "<string>" }
```

### `CONFIG_ERROR`

```json
{ "path": "<path|null>", "hint": "<hint|null>" }
```

### `DB_LOCKED`

```json
{ "sqlite_code": "<DatabaseBusy|DatabaseLocked|null>", "retry_after_ms": 200 }
```

## Exit Code Rules

- 致命エラーは exit code != 0
- `sync --check` は `result.valid = false` のとき `status = "warning"` かつ exit code 1
- `sync` は blocking な `feeds.yaml` validation error があるとき `CONFIG_ERROR` で失敗する
- `sync` の fetch / parse 失敗は致命ではなく、exit code 0 のまま `result.errors` に積む
- `BrokenPipe` は下流の早期終了として扱い、exit code 0 で終了する

## Non-Fatal Sync Errors

`picofeedr sync` の `result.errors` 配列では、少なくとも次のコードを使う。

- `FETCH_FAILED`
- `PARSE_FAILED`

## References

- envelope 全体は `doc/spec/cli.md` を参照する
- 命名規約は `doc/spec/api-naming.md` を参照する
