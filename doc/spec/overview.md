# A. Backend / CLI 仕様（規約）

## A0. ゴール

- RSS/Atom取得・正規化・検索・状態更新・永続化（SQLite）を担当
- **ローカル完結**：ネットワークは `sync` のみ。閲覧/状態更新はローカルSQLiteのみ
- **シンプル優先**：都度CLI起動（1コマンド=1操作）で成立することを最優先
- **依存最小**：DB=SQLite。実装言語はRust想定（外部サービスや常駐前提を置かない）
- **UI差し替え可能**：Emacs/TUI/GUI等、任意のフロントがCLIを叩けば同じ機能を使える
- **タグ中心設計**：unread含むすべての状態をタグで管理
  - 予約タグは `unread` のみ。`star` は通常タグとして扱う
  - `manage_unread = false` のときは unread タグ自動付与を停止できる
- **設定ファイル駆動**：取得対象・タグ継承・自動タグルールは **feeds.yaml が唯一の真実**
- **DBの役割を限定**：SQLiteは「取得結果（entries/tags/content）」のみを保持する（YAMLの内容・ルール自体は保存しない）

## A1. 目的 / 非目的

### 目的

- RSS/Atomの取得
- 正規化データをSQLiteへ保存（単一writer）
- 検索・ソート・ページングをバックエンド側で完結
- タグベースの状態管理：unread/カスタムタグ
- 設定ファイル（YAML）からのフィード管理
- CLI Mode（都度実行）を基本とする

### 非目的（当面）

- フィードのCRUD操作（設定ファイルを直接編集）
- 古いエントリの自動削除（SQLiteを直接操作）
- 多クライアントの厳密同期
- リモート公開前提の認証・権限管理
- 常駐RPCサーバーや外部APIの提供

## A2. 実行形態

### A2.1 CLI Mode（デフォルト）

- `picofeedr <command>` を都度起動し、主要な結果は **標準出力（stdout）** に出す
  - 出力形式は `--output json|plain`（または設定）で切り替える
- `--output json`（機械可読）
  - 成功：exit code 0 + JSON（共通envelope）
  - 致命：exit code != 0 + JSON（共通envelopeの `error`）
  - パイプ先が早期終了して `stdout` が `BrokenPipe` になった場合は、致命扱いにせず exit code 0 で終了する
  - `--debug` の詳細は **標準エラー（stderr）** に寄せる（通常は出さない）
- `--output plain`（対話向け）
  - 成功/失敗ともに、人間向け表示を stdout/stderr に出す
  - 形式は table / kv / log の3カテゴリで定義する
  - 一覧系はタブ区切り、結果系は `key: value`、`sync` は `sync:* key=value ...` の log-oriented 形式
  - `sync` は feed 単位の進捗と最終要約を stdout に逐次出力し、詳細な error line は stderr に出す
  - `stdout` の `BrokenPipe` は非致命として扱い、通常は無出力で終了する（`--debug` 時のみ stderr に診断を出してよい）
  - `--help` は plain 前提でよい（機械可読契約の対象外）

**コマンド一覧：**

```
picofeedr sync                      # 同期実行
picofeedr status                    # DB状態メタデータ
picofeedr list [--query <q>] [--sort <order>] [--limit <n>] [--cursor <token>] [--id]
                                    # エントリ一覧（sort/paging は doc/spec/pagination.md 参照）
picofeedr view <entry_id>           # エントリ詳細
picofeedr mark <operation> <ids>... # 状態更新（read|unread|tag --add/--remove）
                                    # ids は単独の `-` で stdin（UTF-8、空白区切り、1行1ID推奨）も可
picofeedr tags                      # タグ一覧
picofeedr feeds [--id]              # フィード一覧
picofeedr version                   # バージョン情報
picofeedr sync --check              # 同期設定の静的妥当性検証（DB非依存）

```

`mark read|unread|tag` は1件以上のID、または単独の `-` を受け付ける。
`-` 指定時は標準入力をUTF-8の空白区切りトークンとして読み、先頭に1個だけあるUTF-8 BOMを除去する。連続する空白と先頭・末尾の空白は無視する（space/tab/改行等、CRLF可）。1行1IDを推奨するが、同一行のspace/tab区切りも受け付ける。`-` と明示IDの混在、`-` の複数指定、stdin解決後のID 0件は `CONFIG_ERROR` とする。

**共通フラグ：**

```
--config <path>     # config.toml のパス（デフォルト: ~/.config/picofeedr/config.toml）
--storage-root <path>   # ストレージルート上書き（db.sqlite と data/ を含む）
--output <json|plain> # 出力形式（デフォルト: plain）
--debug             # デバッグ情報をstderrに出す（json出力を壊さない）
```
