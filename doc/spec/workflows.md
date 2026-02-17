# C. ユーザーワークフロー

## C1. 初期セットアップ

```bash
# 1. 設定ファイル作成
mkdir -p ~/.config/picofeedr
cp /usr/share/picofeedr/config.example.toml ~/.config/picofeedr/config.toml
cp /usr/share/picofeedr/feeds.example.yaml ~/.config/picofeedr/feeds.yaml

# 2. feeds.yaml 編集
vim ~/.config/picofeedr/feeds.yaml

# 3. 初回同期
picofeedr --output json sync

# 4. エントリ確認
picofeedr --output json list --query unread
```

## C2. 日常利用

```bash
# 朝：同期
picofeedr sync

# Emacsで閲覧
emacs -f picofeedr

# または CLI で確認
picofeedr --output json list --query "unread tag:security" | jq '.result.items[] | {entry_id, title}'
picofeedr --output json view <entry_id>
picofeedr --output json mark read <entry_id>

# 注：本文（content）が無い/取得しない運用の場合は、
# EntryDetail の `link` を外部ブラウザ等で開くのだ。
```

## C3. フィード追加

```bash
# 1. feeds.yaml を編集
vim ~/.config/picofeedr/feeds.yaml
```

```yaml
# 新規追加:
feeds:
  tech:
    programming:
      rust:
        feeds:
          - url: https://new-blog.example.com/feed.xml
            tags: [new]
```

```bash
# 2. 設定の静的妥当性確認
picofeedr --output json feeds --config-check

# 3. 同期（自動的に新規フィードが追加される）
picofeedr --output json sync
```

## C4. 古いエントリの削除（直接SQL）

```bash
# 30日以上前の既読エントリを削除
sqlite3 ~/.local/share/picofeedr/db.sqlite <<'EOF'
DELETE FROM entries
WHERE id IN (
  SELECT e.id
  FROM entries e
  WHERE e.published_at < strftime('%s', 'now', '-30 days')
    AND NOT EXISTS (
      SELECT 1
      FROM entry_tags et
      JOIN tags t ON et.tag_id = t.id
      WHERE et.entry_pk = e.id
        AND t.name = 'unread'
    )
);
EOF
```

---
