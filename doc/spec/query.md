# 検索クエリ言語

## A7. 検索クエリ言語

### A7.1 Phase 4（タグフィルタのみ、MVP）

**サポート構文：**

- `unread` - `tag:unread` のショートカット
- `star` または `starred` - `tag:star` のショートカット
- `tag:security` - 指定タグを持つエントリ
- `-tag:misc` - 指定タグを持たないエントリ
- スペース区切りは AND 条件

**例：**

```

feeder list --query "unread tag:security -tag:misc"

# → 未読 AND securityタグあり AND miscタグなし

```

### A7.2 Phase 6（拡張クエリ）

**追加構文：**

- `text:"keyword"` - 全文検索（FTS5、要検討：日本語トークナイズ等）
- `feed:123` または `feed:"Feed Title"` - 特定フィード
- `before:2026-01-01` / `after:2025-12-01` - 日付範囲（`date = COALESCE(published_at, updated_at, first_seen_at)` に対して適用）

**例：**

```

feeder list --query 'unread text:"rust" after:2026-01-01'

```

### A7.3 SQL生成

**タグフィルタの例：**

```

-- tag:security EXISTS ( SELECT 1 FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE et.entry_id = entries.id AND t.name = 'security' )

-- -tag:misc NOT EXISTS ( SELECT 1 FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE et.entry_id = entries.id AND t.name = 'misc' )

```
