# 検索クエリ言語

## A7. 検索クエリ言語

### A7.1 Phase 4（タグフィルタのみ、MVP）

**サポート構文：**

- `unread` - `tag:<unread_tag>` のショートカット（`unread_tag` は設定。デフォルト `unread`）
- 予約トークンは `unread` のみ。`star` は `tag:star` で表現するのだ
- `tag:security` - 指定タグを持つエントリ
- `-tag:misc` - 指定タグを持たないエントリ
- スペース区切りは AND 条件
- `feed:` / `title:` / `after:` / `before:` はそれぞれ 1 回のみ指定可能（複数指定は `INVALID_QUERY`）

**例：**

```

picofeedr list --query "unread tag:security -tag:misc"

# → 未読 AND securityタグあり AND miscタグなし

```

### A7.2 Phase 5（拡張クエリ）

**追加構文：**

- `title:"keyword"` - タイトル部分検索（暫定仕様、後で全文検索方針と合わせて再検討）
- `feed:123` または `feed:"Feed Title"` - 特定フィード
- `before:2026-01-01` / `after:2025-12-01` - 日付範囲（`date = COALESCE(published_at, updated_at, first_seen_at)` に対して適用）

### A7.2.1 トークン化（クォート・エスケープ）

- クエリは空白区切りでトークン化する
- `"` で囲んだ区間では空白を値として保持する
- クォート内では `\"` を `"`、`\\` を `\` として扱う
- クォートが閉じていない場合は `INVALID_QUERY` とする

**例：**

```

picofeedr list --query 'unread title:"rust" feed:"Rust Blog" after:2026-01-01'

```

### A7.3 SQL生成

**タグフィルタの例：**

```

-- tag:security EXISTS ( SELECT 1 FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE et.entry_id = entries.id AND t.name = 'security' )

-- -tag:misc NOT EXISTS ( SELECT 1 FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE et.entry_id = entries.id AND t.name = 'misc' )

```
