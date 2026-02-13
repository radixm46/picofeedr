# エラー仕様（CLI）

## A10. エラー仕様（CLI）

### A10.1 失敗時のJSON（`--output json`）

```json
{ "success": false, "severity": "error", "result": null, "error": { "code": "DB_LOCKED", "message": "Database is locked, please retry", "retryable": true, "details": null }, "meta": {"api_version": "<string>", "schema_version": <int>, "generated_at": <epoch>} }
```

**exit code：**

- 致命（fatal）は **exit code != 0** + 上記 JSON なのだ。
- `sync` の fetch/parse 失敗は致命ではなく、exit code は 0 のまま `result.errors` に積むのだ。
- `feeds --config-check` は validation report を `success=true` で返し、`result.valid=false` のときは `severity=warn` かつ exit code 1 を返すのだ。
- `stdout` への書き込みで `BrokenPipe` が起きた場合は、下流の早期終了として扱い、exit code 0 で終了するのだ。

### A10.2 error code 例

- `ENTRY_NOT_FOUND`
- `INVALID_QUERY`
- `DB_LOCKED`
- `CONFIG_ERROR`
- `DB_ERROR`
- `IO_ERROR`
- `SERIALIZATION_ERROR`
- `INTERNAL`

### A10.3 sync errors（非致命）

`picofeedr sync` の `result.errors` 配列で使うコード例なのだ。

- `FETCH_FAILED`
- `PARSE_FAILED`
