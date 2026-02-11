# C. ユーザーワークフロー

## C1. 初期セットアップ

```bash
# 1. 設定ファイル作成
mkdir -p ~/.config/feeder
cp /usr/share/feeder/config.example.toml ~/.config/feeder/config.toml
cp /usr/share/feeder/feeds.example.yaml ~/.config/feeder/feeds.yaml

# 2. feeds.yaml 編集
vim ~/.config/feeder/feeds.yaml

# 3. 初回同期
feeder sync --output json

# 4. エントリ確認
feeder list --output json --query unread
```

## C2. 日常利用

```bash
# 朝：同期
feeder sync

# Emacsで閲覧
emacs -f feeder

# または CLI で確認
feeder list --output json --query "unread tag:security" | jq '.data.items[] | {id, title}'
feeder view --output json 123
feeder mark --output json read 123

# 注：本文（content）が無い/取得しない運用の場合は、
# EntryDetail の `link` を外部ブラウザ等で開くのだ。
```

## C3. フィード追加

```bash
# 1. feeds.yaml を編集
vim ~/.config/feeder/feeds.yaml
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
feeder feeds --output json --config-check

# 3. 同期（自動的に新規フィードが追加される）
feeder sync --output json
```

## C4. 古いエントリの削除（直接SQL）

```bash
# 30日以上前の既読エントリを削除
sqlite3 ~/.local/share/feeder/db.sqlite <<'EOF'
DELETE FROM entries
WHERE id IN (
  SELECT e.id
  FROM entries e
  WHERE e.published_at < strftime('%s', 'now', '-30 days')
    AND NOT EXISTS (
      SELECT 1
      FROM entry_tags et
      JOIN tags t ON et.tag_id = t.id
      WHERE et.entry_id = e.id
        AND t.name = 'unread'
    )
);
EOF
```

---
