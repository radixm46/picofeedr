# 検索クエリ仕様

## Scope

この文書は `picofeedr list --query` で受理する検索クエリの文法と制約を定義する。  
ページング契約は `doc/spec/pagination.md`、日付検索の詳細は `doc/spec/query-date.md` を正本とする。

## Supported Tokens

現行で受理するトップレベルトークンは次のとおり。

- `unread`
- `tag:<expr>`
- `-tag:<expr_without_not>`
- `feed:<feed_id>` または `feed:"<feed_title>"`
- `title:"<keyword>"`
- `after:<date_or_duration>`
- `before:<date_or_duration>`

## Top-Level Rules

- `unread` は `tag:<unread_tag>` のショートカット
- `feed:` / `title:` / `after:` / `before:` はそれぞれ 1 回のみ指定可能
- 同じ種類のトップレベルトークンを複数回使った場合は `INVALID_QUERY`
- `after` と `before` を同時指定した場合は `after < before` を必須とする

## Tag Expression

### Operators

- `AND`, `OR`, `NOT` を受理する
- alias として `&`, `|`, `!` を受理する
- 優先順位は `NOT > AND > OR`
- 括弧 `(` `)` で優先順位を明示できる
- 暗黙ANDを許可する

### Grammar

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

### `-tag:` Rules

- `-tag:` はトップレベル `NOT` のエイリアス
- `-tag:` の内部では `NOT` / `!` を禁止する
- `-tag:!A` と `-tag:NOT A` は `INVALID_QUERY`
- `tag:rust -tag:rust` のような直接矛盾は `INVALID_QUERY`

## Tokenization

- クエリは空白区切りでトークン化する
- `"..."` 内の空白は保持する
- クォート内では `\"` を `"`、`\\` を `\` として扱う
- 未閉じクォートは `INVALID_QUERY`

## Date Filters

- `after:` / `before:` は `YYYY-MM-DD` または `N[d|w|m|y]` を受理する
- 絶対日付と相対 duration の混在を許可する
- 詳細な境界解決規則は `doc/spec/query-date.md` を正本とする

## Evaluation Model

`tag` 式は再帰的に SQL 条件へ変換する。

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

## CLI Notes

- `|` と `&` はシェルで解釈されるので、`--query` は原則シングルクォートで囲う

## Examples

```bash
picofeedr list --query 'tag:A|B|C'
picofeedr list --query 'tag:A&(B|C)'
picofeedr list --query 'tag:("rust news"|tech) -tag:misc'
picofeedr list --query 'after:2026-01-01 before:2w'
```

## Non-Goals

- この文書は cursor の内部構造を定義しない
- この文書は全文検索や未実装演算子を定義しない
