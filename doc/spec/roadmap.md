# MVP 実装順（推奨）

## A11. MVP 実装順（推奨）

### Phase 0：プロジェクト基盤（Week 1前半）

1. プロジェクト構成
2. `config.toml` 読み込み
3. `feeds.yaml` 読み込み（階層パース、タグ継承）
4. Database初期化・マイグレーション
5. 基本的なCLI構造

### Phase 1：タグシステム（Week 1後半）

6. `tags` / `entry_tags` テーブル操作
7. TagManager実装
8. CLI: `feeder tags`
9. テスト

### Phase 2：フィード管理（Week 2前半）

10. `feeds` テーブル操作
11. `feeds.yaml` とDBの同期（reconcileFeeds）
12. CLI: `feeder feeds`
13. CLI: `feeder feeds --config-check`

### Phase 3：同期処理（Week 2後半）

14. RSS/Atom fetch実装（ETag/Last-Modified対応）
15. Entry正規化・保存
16. 自動タグルール適用
17. 並列fetch実装（worker pool）
18. CLI: `feeder sync`

**マイルストーン1：基本同期完成**

### Phase 4：エントリ一覧・タグフィルタ（Week 3）

19. Query parser（タグフィルタのみ）
20. entries.query 実装（カーソルページング）
21. CLI: `feeder list`
22. CLI: `feeder view`
23. CLI: `feeder mark`

**マイルストーン2：実用可能（ここまでで120 feedsでも快適）**

### Phase 5：クエリ拡張（Week 4前半〜後半）

28. FTS5テーブル作成・更新
29. クエリ言語拡張（text:, feed:, before:/after:）
30. CLI: 拡張クエリ対応
31. テスト

**マイルストーン3：高度な検索完成**

### Phase 6：JSON-RPC Mode（Week 5-6）

32. JSON-RPC server実装（stdio）
33. `sync.run` with notifications
34. 他のメソッド実装
35. Emacsクライアント（RPC版）
36. モード切り替え機能

**マイルストーン4：RPC Mode完成**

---
