# 検索クエリ言語

## A7. 検索クエリ言語

### A7.1 サポート構文（現行）

- `unread` - `tag:<unread_tag>` のショートカット（`unread_tag` は設定。デフォルト `unread`）
- `tag:<expr>` - タグ論理式
- `-tag:<expr_without_not>` - 除外式の糖衣構文（`NOT(tag:<expr_without_not>)` と等価）
- `feed:123` または `feed:"Feed Title"` - 特定フィード
- `title:"keyword"` - タイトル部分検索（`LIKE '%keyword%'`）
- `before:YYYY-MM-DD` / `after:YYYY-MM-DD` - 日付範囲（`date = COALESCE(published_at, updated_at, first_seen_at)`）
- `feed:` / `title:` / `after:` / `before:` はそれぞれ 1 回のみ指定可能（複数指定は `INVALID_QUERY`）
- `after` と `before` を同時指定した場合は `after < before` を必須とする

### A7.2 tag式の文法

#### A7.2.1 演算子

- 基準演算子: `AND`, `OR`, `NOT`（大文字小文字は不問）
- alias: `&` = `AND`, `|` = `OR`, `!` = `NOT`
- 優先順位: `NOT` > `AND` > `OR`
- 括弧 `(` `)` で優先順位を明示可能
- 暗黙ANDを許可（例: `A B` は `A AND B`）

#### A7.2.2 EBNF（簡易）

```ebnf
TagExpr      ::= OrExpr
OrExpr       ::= AndExpr ( OrOp AndExpr )*
AndExpr      ::= UnaryExpr ( (AndOp | ImplicitAnd) UnaryExpr )*
UnaryExpr    ::= (NotOp)* Primary
Primary      ::= TagLiteral | "(" OrExpr ")"

OrOp         ::= "OR" | "|"
AndOp        ::= "AND" | "&"
NotOp        ::= "NOT" | "!"
ImplicitAnd  ::= <adjacent terms>

TagLiteral   ::= BareLiteral | QuotedLiteral
BareLiteral  ::= <whitespace / operator / parenthesis 以外の文字列>
QuotedLiteral::= '"' ( '\\"' | '\\\\' | <other> )* '"'
```

#### A7.2.3 `-tag:` の制約

- `-tag:` はトップレベル `NOT` のエイリアスとして扱う
- `-tag:` の内部では `NOT/!` を禁止する（`INVALID_QUERY`）
- 受理例:
  - `-tag:A|B|C`（`NOT (A OR B OR C)`）
  - `tag:A&B&C -tag:D|E`（`A AND B AND C AND NOT (D OR E)`）
- 非対応（`INVALID_QUERY`）:
  - `-tag:!A`
  - `-tag:NOT A`
- `tag:rust -tag:rust` のような直接矛盾は `INVALID_QUERY`

### A7.3 トークン化（全体クエリ）

- クエリは空白区切りでトークン化する
- `"..."` 内の空白は保持する
- クォート内では `\"` を `"`、`\\` を `\` として扱う
- 未閉じクォートは `INVALID_QUERY`

### A7.4 SQL生成

`tag` 式は再帰的に SQL へ変換する。

```sql
-- Tag(x)
EXISTS (
  SELECT 1 FROM entry_tags et
  JOIN tags t ON et.tag_id = t.id
  WHERE et.entry_pk = e.id AND t.name = ?
)

-- Not(expr)
NOT (<expr>)

-- And([a,b,...])
(<a>) AND (<b>) ...

-- Or([a,b,...])
(<a>) OR (<b>) ...
```

### A7.5 使用例

```bash
picofeedr list --query 'tag:A|B|C'
picofeedr list --query 'tag:A&B'
picofeedr list --query 'tag:A&(B|C)'
picofeedr list --query 'tag:(A OR B) AND !C'
picofeedr list --query 'tag:("rust news"|tech) -tag:misc'
```

### A7.6 CLI利用時の注意

- `|` と `&` はシェルで解釈されるので、`--query` は原則シングルクォートで囲う
- 例: `--query 'tag:(A|B)&!C'`

### A7.7 日付検索拡張（Draft）

- 相対日付を含む拡張仕様は `doc/spec/query-date.md` を参照する
- Draft 方針:
  - `after:` / `before:` は `YYYY-MM-DD` または `N[d|w|m|y]` を受理
  - 絶対/相対の混在を許可
  - 相対値は同一クエリ内で固定した `now` を基準に解決
  - 絶対日付 (`YYYY-MM-DD`) もローカル日付 0:00 として解決
  - 相対値の境界はローカル日付の 0:00 基準（そのため `0d` / `0w` / `0m` / `0y` は同義）
  - 解決後 `after >= before` は `INVALID_QUERY`
