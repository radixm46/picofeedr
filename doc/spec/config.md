# 設定ファイル（2層構造）

## A3. 設定ファイル（2層構造）

### A3.1 config.toml（CLI動作設定）

アプリケーションの**動作方法**を定義（変更頻度：低）

```
# ~/.config/picofeedr/config.toml

unread_tag = "unread"       # 未読タグ名（新規取り込み時に付与）

[storage]
root_dir = "~/.local/share/picofeedr"
content_store = "db"      # db | fs | none

[sync]
parallel = 5              # 並列fetch数
timeout = 30              # HTTP timeout（秒）
user_agent = "picofeedr/0.1.0"
retry_count = 3
retry_delay = 5

[query]
default_limit = 100
max_limit = 1000

[feeds]
source = "~/.config/picofeedr/feeds.yaml"

[cli]
output = "plain"          # json | plain（CLIフラグがあればそちらを優先）

[log]
level = "info"            # error | warn | info | debug | trace（主にstderr向け）

```

`content_store` は `entry_contents.storage` の値として扱うのだ。

`storage.root_dir` から `db.sqlite` と `data/` を導出するのだ。  
つまり DB は `storage.root_dir/db.sqlite`、ファイル保存先は `storage.root_dir/data` になるのだ。

`content_store = "fs"` の場合、`entry_contents.ref` は hash key（例：sha256 hex）で、実際の保存パスは `storage.root_dir/data` と導出ルールから決めるのだ（レコードにはパスを持たない）。

`unread_tag` は新規取り込み時に付与する未読タグ名なのだ（既読化はこのタグを外す）。

`cli.output` は CLI の出力形式のデフォルト値なのだ。対話用途は `plain`、UI/自動化用途は `json` を推奨するのだ（詳細は `doc/spec/cli.md`）。

`log.level` はデバッグ/トレース出力の粒度の目安なのだ。ログは stdout を汚さないため、原則 stderr に寄せるのだ（詳細は `doc/spec/overview.md`）。

`query.default_limit` は `list --limit` 未指定時の既定件数なのだ。`query.max_limit` は安全上限で、`--limit` がこれを超える場合は `INVALID_QUERY` になるのだ。`default_limit` / `max_limit` はどちらも 1 以上で、`default_limit <= max_limit` を必須とするのだ。

### A3.2 feeds.yaml（フィード一覧・自動タグ）

**データの内容**を定義（変更頻度：高）

```
# ~/.config/picofeedr/feeds.yaml

picofeedr:
  auto_tags:
    - title_regex: '(?i)CVE-\d{4}-\d+'
      add_tags: [cve, security-alert]
    - title_contains: [vulnerability, exploit, 0-day]
      add_tags: [security-alert]

  tech:
    tags: [tech]
    auto_tags:
      - title_contains: [release]
        add_tags: [release-note]
    programming:
      tags: [programming]
      rust:
        auto_tags:
          - title_contains: [unsafe]
            add_tags: [rust-unsafe]
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

```

**階層構造とタグ継承：**

* 親グループのタグは子グループに継承される
* 例：`tech.programming.rust` のフィードは `[tech, programming, rust]` タグを持つ
* トップレベルは `picofeedr` のみを解釈し、それ以外のキーは無視する

## A9. 自動タグ（feeds.yaml）

### A9.1 ルール定義

`auto_tags` は `picofeedr.auto_tags` だけでなく、任意のグループ配下にも定義できるのだ。

親グループの `auto_tags` は子グループ・配下 feed に継承されるのだ。

ある feed に適用される `auto_tags` は「親から継承されたルール + 同一/子グループで追加されたルール」の合成結果なのだ。

`title_contains` が複数要素の場合は OR マッチ（いずれかを含めば一致）として扱うのだ。
`title_contains` の一致判定は大文字小文字を区別しない（case-insensitive）挙動なのだ。

`priority` は任意項目なのだ。通常運用では指定しなくてもよく、未指定時は既定値（0）として扱うのだ。

### A9.2 適用タイミング

- 新規エントリの取り込み時のみ（`sync` 実行時）
- ルール変更を過去分に遡及しない

### A9.3 タグ付与順序

1. フィード階層から継承されたタグ
2. `auto_tags` ルール（定義順。`priority` 指定時は優先度順）
3. `unread_tag`（常に最後）
