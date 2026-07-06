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
- `after:<date_or_duration>`
- `before:<date_or_duration>`
- `<term>` または `"<term>"`（title への検索語。詳細は Word Search）
- `-<term>` または `-"<term>"`（否定の検索語。title が term を含まないことを要求）
- `(<term_expr>)` および `-(<term_expr_without_not>)`（term のブール式グループ。詳細は Word Search）

## Top-Level Rules

- `unread` は小文字完全一致のキーワードで、`unread_tag` のショートカット。`Unread` などは通常の term
- `unread` は各1回制限の対象外で反復可能。重複は dedupe される。unread 管理が無効なときは no-op（フィルタなしと同義）
- フィルタ prefix は `tag:` `-tag:` `feed:` `after:` `before:` の5種のみ。小文字完全一致だけをフィルタとして解釈する（`Tag:rust` はフィルタにならず、ASCII `:` を含む未知 prefix として `INVALID_QUERY`。リテラル検索は `"Tag:rust"` と書く）
- フィルタにも `unread` にも該当しないトークンは検索語（term）またはグループとして扱う
- ただしクォートされていないトークンが ASCII `:` を含む場合、フィルタ prefix で始まらなければ `INVALID_QUERY` とし、`details.hint` で引用符による literal 検索を案内する
- `(` で始まるトークンはブール式グループ（Word Search / Term Groups を参照）
- `-` で始まるトークンは、`-tag:` で始まる場合はタグ否定フィルタ、`-(` で始まる場合は否定グループ、それ以外は否定 term として解釈する（`-foo:bar` のような形は ASCII `:` を含む未知 prefix として `INVALID_QUERY`）
- 括弧式トークン（`(` `-(` `tag:(` `-tag:(` で始まるトークン）は、対応する閉じ括弧 `)` まで空白を跨いで1トークンとして読む（`tag:( A | B )` は `tag:(A|B)` と等価。詳細は Tokenization）
- 括弧を伴わないフィルタ値は空白で終端する。`tag:` / `-tag:` が後続トークンを式として吸収することはない（`tag:A B` の `B` はタグではなく term）
- トークン全体が演算子（`AND` `OR` `NOT` `&` `|` `!`）に一致する場合は `INVALID_QUERY` とし、`details.hint` で引用符による literal 検索を案内する（`tag:A | B` のような空白分割の書き間違いを silent に title 検索へ落とさないため）
- bare operator エラーになるのは大文字完全一致の `AND` `OR` `NOT` と記号 `&` `|` `!` のみ。それ以外の `or` `Or` `oR` `and` `not` などはトップレベルでは通常の term
- そのため `tag:a Or b` は tag `a` と term `"Or"` と term `"b"` の暗黙 AND として受理する
- クォートされていない bare term に `|` / `&` を含む場合、または先頭が `!` の場合も `INVALID_QUERY` とし、literal 検索は quoted term（例: `"a|b"` / `"!foo"`）で表す
- `tag:` / `-tag:` / `feed:` / `after:` / `before:` はそれぞれ 1 回のみ指定可能
- 同じ種類のフィルタトークンを複数回使った場合は `INVALID_QUERY`。`tag:` / `-tag:` は `details.hint` で単一式への統合を案内する（`tag:a tag:b` は `tag:(a&b)`、`-tag:a -tag:b` は `-tag:(a|b)` と等価）。`feed:` / `after:` / `before:` は `remove_duplicate_filter` を返す
- `after` と `before` を同時指定した場合は `after < before` を必須とする
- トークン間は暗黙 AND で合成する

### 未知 prefix をエラーにする理由

- typo（`tga:rust` など）が検索語へ silent に化けて「0件の理由が分からない」状態を防ぐ
- 将来フィルタキーワードを追加しても既存クエリの意味が変わらない（エラーだったトークンが動くようになるだけの追加的変更に保てる）

### Grammar

```ebnf
Query        ::= Token ( WS Token )*
Token        ::= Filter | Term | NegTerm | TermGroup | NegTermGroup
Filter       ::= "unread" | "tag:" TagExpr | "-tag:" TagExprWithoutNot
               | "feed:" FeedValue
               | "after:" DateOrDuration | "before:" DateOrDuration
TermGroup    ::= "(" TermExpr ")"
NegTermGroup ::= "-(" TermExprWithoutNot ")"
NegTerm      ::= "-" Term
Term         ::= QuotedTerm | BareTerm
QuotedTerm   ::= '"' ( '\\"' | '\\\\' | <quote / backslash 以外の文字> )+ '"'
BareTerm     ::= <whitespace / '"' / ASCII ':' / '|' / '&' を含まない1文字以上の文字列。先頭は '-' '(' '!' 以外>
```

- ASCII `:` を含むが `Filter` に一致しないトークンは文法エラー（`INVALID_QUERY`）
- `-tag:` で始まるトークンは常に `Filter` として解釈する（`NegTerm` / `NegTermGroup` より優先）
- `TermExpr` / `TermExprWithoutNot` の文法は Tag Expression の `TagExpr` と同一（`TagLiteral` を term リテラルに読み替える）
- `WS` はトークン区切りだが、括弧式トークン内の空白はトークンを終端しない（Tokenization を参照）

## Word Search

- 検索語（term）は entry title への部分文字列マッチ条件になる
- 複数 term は暗黙 AND（すべての term を含む title のみマッチ）
- ASCII アルファベットは case-insensitive、それ以外の文字は exact match（SQLite `LIKE` の既定挙動に一致）
- term はリテラルとして扱う。`%` / `_` にワイルドカードの意味はない
- クォート（`"..."`）の役割は2つ: 空白を含むフレーズ化と、演算子解釈の抑止（`"tag:rust"` や `"unread"` は検索語になる）
- quoted term 内の `"` は `\"` として escape する必要がある。閉じクォート後に残余文字が続く token（`"a"b"c"`）は `INVALID_QUERY`
- 空文字列の term（`""`）は `INVALID_QUERY`。`feed:""` や `after:""` などの quoted scalar value と、グループ内の quoted リテラル（`(""|foo)`）も同様
- term リテラルは1クエリあたり合計最大 32 個（グループ内のリテラルも算入）。超過は `INVALID_QUERY`
- term およびグループは `query_hash` に含まれ、カーソル再現性の対象となる（`doc/spec/pagination.md`）
- 選言（OR）や式内否定が必要な絞り込みはグループ（Term Groups）で行う

### Negated Terms

- `-<term>` / `-"<term>"` は否定 term。title がその term を含まない entry にマッチする
- 否定として解釈するのはトークン先頭の `-` 1個のみ。`--foo` は `INVALID_QUERY`
- term 中間・末尾のハイフンはリテラル（`state-of-the-art` は1つの肯定 term）
- 先頭がハイフンの語をリテラル検索するにはクォートする（`"-rc1"`）
- `-` 単独のトークンは `INVALID_QUERY`
- 否定 term のみのクエリも許可する
- 同一 term の肯定・否定の併用（`foo -foo`。ASCII case-fold 後の一致で判定）は直接矛盾として `INVALID_QUERY`（`tag:rust -tag:rust` と同じ扱い）
- 部分文字列関係による意味上の矛盾（`foobar -foo` は常に0件）は検出しない
- 否定 term も肯定 term と合わせて term 数の上限（32個）に数える
- title が `NULL` の entry では、各 term リテラルを偽として評価する（肯定 term にマッチせず、`-<term>` / `-(<expr>)` にはマッチする）

### Term Groups

- `(<expr>)` はブール式グループ。文法・演算子・優先順位は Tag Expression と同一で、葉が term リテラルになる（例: `((A&B)|"c d")`）
- `-(<expr>)` はグループ全体の否定糖衣。`-tag:` と同じく内部で `NOT` / `!` を禁止する（違反は `INVALID_QUERY`）
- グループは対応する閉じ括弧 `)` まで空白を跨げる（`( "machine learning" | ML )`）。空白は語の区切りとしてのみ働き、隣接は暗黙 AND（`(rust cli)` は `(rust&cli)` と等価）
- `|` `&` `!` が演算子として解釈されるのはグループおよび `tag:` 式の内部のみ。トップレベルの素の term で `|` / `&` または先頭 `!` を literal として検索するには quote する（例: `"a|b"`）
- グループ内の `-x` は否定ではなくリテラル term。式内否定は `!x` または `NOT x` を使う
- `(` がグループ開始として解釈されるのはトークン先頭（および `-(`）のみ。`Rust(2024)` は素の term
- グループ内の unquoted リテラルに ASCII `:` を含む場合も Top-Level Rules と同様に `INVALID_QUERY`
- グループには Tag Expression と同じ AST 深さ上限（最大 16）を適用する。上限は書かれた括弧ネスト（冗長括弧を含む）と `NOT` / `!` 連鎖に対してパース中に適用する。リテラル数は Word Search の term 総数上限（32個）に算入して制約する
- 直接矛盾の検出は単独 term 同士（`foo -foo`）のみ。単一 term へ畳まれるグループ（`(foo)` / `-(foo)`）は bare term に正規化されるため、直接矛盾検出と `query_hash` は bare form と同一。複数 term やネストを含むグループの恒偽式は検出せず0件を返す

## Tag Expression

### Operators

- 演算子セットは記号 `&` `|` `!` と語形 `AND` `OR` `NOT` のみで固定する。追加のエイリアスは提供しない
- 式内部（`tag:(...)` / term グループ）の語形演算子は case-insensitive
- そのため、式内部では `and` `or` `not` という bare literal は演算子として予約される。これらをタグ名として検索するには quote する（例: `tag:"or"` / `tag:("and"|rust)`）
- 語形を bare literal ではなく演算子として予約するのは、`tag:(a AND b)` の `AND` が tag literal へ silent に化け、3タグの暗黙 AND として解釈されることを防ぐため。これは未知 prefix をエラーにする理由と同じく、入力ミスを別の意味へ落とさないため
- 優先順位は `NOT > AND > OR`
- 括弧 `(` `)` で優先順位を明示できる
- 暗黙ANDを許可する
- 括弧で開く式（`tag:(...)` / `-tag:(...)`）内では空白を自由に使える（`tag:( rust | cli )`）
- 括弧を伴わない形（`tag:A|B`）は空白で終端する（`tag:A | B` は bare operator エラー）
- `tag:(a)(b)` は1つの tag 式内の暗黙 AND（`tag:(a&b)` と等価）。`tag:(a) (b)` は tag `a` とトップレベル term グループ `(b)` の暗黙 AND
- リテラル自体に空白を含めるには quoted リテラルを使う（`tag:("rust news"|tech)`）
- 空の quoted リテラル（`tag:""`）は `INVALID_QUERY`
- tag 式の literal は1式あたり最大 64 個。AND/OR のフラット化と dedupe による正規化後にカウントする
- AST 深さ上限は最大 16。書かれた括弧ネスト（冗長括弧を含む）と `NOT` / `!` 連鎖に対してパース中に適用する

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
QuotedLiteral::= '"' ( '\\"' | '\\\\' | <quote / backslash 以外の文字> )+ '"'
```

### `-tag:` Rules

- `-tag:` は `tag:` 式全体を否定する糖衣（トップレベル `NOT` のエイリアス）
- ブール式（選言・ネスト・式内否定）は括弧原子（`tag:(...)` と term グループ `(...)`）の中に閉じ、トップレベルは平坦な暗黙 AND とする
- トップレベルの否定糖衣は `-tag:` / `-<term>` / `-(<expr>)` のみ。`feed:` などの他フィルタには提供しない
- `-tag:` の内部では `NOT` / `!` を禁止する
- `-tag:!A` と `-tag:NOT A` は `INVALID_QUERY`
- 直接矛盾の検出は、正規化（AND/OR のフラット化・dedupe）後のトップレベル `AND` 直下にある `Tag(x)` と `Not(Tag(x))` の直接ペアのみを対象にする
- そのため `tag:rust -tag:rust` と `tag:(rust & !rust)` は `INVALID_QUERY`
- 一方、`tag:a -tag:(a|b)` のように `Not(...)` の中がネスト式である恒偽式は直接矛盾としては検出せず、評価結果として0件を返す。これは Term Groups の直接矛盾検出が単独 term 同士に限られるのと同じ考え方

## Tokenization

- クエリは空白区切りでトークン化する。連続する空白は単一の区切りとして扱う
- `"..."` 内の空白は保持する
- クォート内では `\"` を `"`、`\\` を `\` として扱う
- それ以外の `\` エスケープは `INVALID_QUERY`。`details.hint` は `escape_backslash_as_double_backslash` を返す
- 空白・演算子・特殊文字をリテラルとして扱わせる手段はクォート（`"..."`）のみ。クォート外の `\` は通常のリテラル文字であり、`a\b` は合法な bare term / tag literal
- そのため `\&` `\|` `\(` `\)` などのバックスラッシュエスケープは提供しない。演算子や特殊文字だけをリテラル検索したい場合は、バックスラッシュを付けずにクォート内へ書く（例: `"&"`）
- トークンが式開始 prefix（`(` `-(` `tag:(` `-tag:(`）で始まる場合、対応する閉じ括弧 `)` まで空白でトークンを終端しない
- 括弧の対応はネストを数える。quoted リテラル内の `(` `)` は数えない
- 括弧が閉じる前にクエリ末尾へ達した場合は `INVALID_QUERY`（unclosed parenthesis）
- 括弧の対応が取れた後も、次の空白までは同一トークンとして読む。`(A|B)x` は暗黙 AND として解釈する（`(A|B)&x` と等価。`tag:(A|B)x` も同様）
- トークン先頭以外の `(` `)` は式モードに入らない（`Rust(2024)` は1つの term。Term Groups を参照）
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

検索語（term）は title への `LIKE` 条件へ変換する。

```sql
-- TermLiteral(x)
(e.title IS NOT NULL AND e.title LIKE ? ESCAPE '\')
-- bind 値: '%' || escape(x) || '%'
-- escape(x): x 中の `\` `%` `_` の直前に `\` を付与する
```

- グループ内の `Not` / `And` / `Or` は tag 式と同じ規則で SQL へ合成する
- 単独の `-<term>` は `NOT (TermLiteral(x))` であり、`(e.title IS NULL OR e.title NOT LIKE ? ESCAPE '\')` と等価

## CLI Notes

- `|` `&` `(` `)` はシェルで解釈されるので、`--query` は原則シングルクォートで囲う
- `-` で始まるクエリも `--query '-nightly'` の形で受理する（`--query=` 形式を必須としない）

## Examples

```bash
picofeedr list --query 'tag:A|B|C'
picofeedr list --query 'tag:A&(B|C)'
picofeedr list --query 'tag:("rust news"|tech) -tag:misc'
picofeedr list --query 'after:2026-01-01 before:2w'
picofeedr list --query 'rust async after:2w'
picofeedr list --query 'rust -nightly -"sponsored post"'
picofeedr list --query 'tag:( rust | cli ) (AI|ＡＩ) -( sponsored | 広告 )'
picofeedr list --query '((rust&async)|"async rust") after:1m'
picofeedr list --query '"state of the art" tag:ml'
picofeedr list --query '"operator:" feed:"Release Notes"'
picofeedr list --query '"unread"'
```

最後の例は `unread` フィルタではなく、語としての `unread` を title から検索する。

## Non-Goals

- この文書は cursor の内部構造を定義しない
- この文書は全文検索や未実装演算子を定義しない
