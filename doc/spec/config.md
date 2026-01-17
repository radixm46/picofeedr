# 設定ファイル（2層構造）

## A3. 設定ファイル（2層構造）

### A3.1 config.toml（CLI動作設定）

アプリケーションの**動作方法**を定義（変更頻度：低）

```
# ~/.config/feeder/config.toml

[database]
path = "~/.local/share/feeder/db.sqlite"

[sync]
parallel = 5              # 並列fetch数
timeout = 30              # HTTP timeout（秒）
user_agent = "feeder/0.1.0"
retry_count = 3
retry_delay = 5

[storage]
content_store = "db"      # db | fs | none
data_dir = "~/.local/share/feeder/data"

[tags]
unread = "unread"         # 未読タグ名
starred = "star"          # スタータグ名

[query]
default_limit = 100
max_limit = 1000

[feeds]
source = "~/.config/feeder/feeds.yaml"

[log]
level = "info"
file = "~/.local/share/feeder/feeder.log"

```

`content_store` は `entry_contents.storage` の値として扱うのだ。

### A3.2 feeds.yaml（フィード一覧・自動タグ）

**データの内容**を定義（変更頻度：高）

```
# ~/.config/feeder/feeds.yaml

feeds:
  tech:
    tags: [tech]
    programming:
      tags: [programming]
      rust:
        tags: [rust]
        feeds:
          - url: https://blog.rust-lang.org/feed.xml
            title: Rust Blog
          - url: https://this-week-in-rust.org/rss.xml
      go:
        tags: [golang]
        feeds:
          - url: https://go.dev/blog/feed.atom
    security:
      tags: [security, important]
      feeds:
        - url: https://security.googleblog.com/feeds/posts/default
        - url: https://krebsonsecurity.com/feed/

  news:
    tags: [news]
    feeds:
      - url: https://news.ycombinator.com/rss
        title: Hacker News
      - url: https://lobste.rs/rss

auto_tags:
  - title_regex: '(?i)CVE-\d{4}-\d+'
    add_tags: [cve, security-alert]
    priority: 10
  - title_contains: [vulnerability, exploit, 0-day]
    add_tags: [security-alert]
    priority: 20

```

**階層構造とタグ継承：**

* 親グループのタグは子グループに継承される
* 例：`tech.programming.rust` のフィードは `[tech, programming, rust]` タグを持つ

## A9. 自動タグ（feeds.yaml）

### A9.1 ルール定義

`auto_tags` は `feeds.yaml` のトップレベルに定義するのだ。

### A9.2 適用タイミング

- 新規エントリの取り込み時のみ（`sync` 実行時）
- ルール変更を過去分に遡及しない

### A9.3 タグ付与順序

1. フィード階層から継承されたタグ
2. `auto_tags` ルール（優先度順）
3. `tags.unread` タグ（常に最後）
