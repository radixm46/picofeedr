# A. Backend / CLI 仕様（規約）

## A0. ゴール

* RSS/Atom取得・正規化・検索・状態更新・永続化（SQLite）を担当
* **ローカル完結**：ネットワークは `sync` のみ。閲覧/状態更新はローカルSQLiteのみ
* **シンプル優先**：都度CLI起動（1コマンド=1操作）で成立することを最優先
* **依存最小**：DB=SQLite。実装言語はRust想定（外部サービスや常駐前提を置かない）
* **UI差し替え可能**：Emacs/TUI/GUI等、任意のフロントがCLIを叩けば同じ機能を使える
* **タグ中心設計**：unread含むすべての状態をタグで管理
  - 予約タグは `unread` のみ。`star` は通常タグとして扱うのだ
* **設定ファイル駆動**：取得対象・タグ継承・自動タグルールは **feeds.yaml が唯一の真実**
* **DBの役割を限定**：SQLiteは「取得結果（entries/tags/content）」のみを保持する（YAMLの内容・ルール自体は保存しない）

## A1. 目的 / 非目的

### 目的

* RSS/Atomの取得
* 正規化データをSQLiteへ保存（単一writer）
* 検索・ソート・ページングをバックエンド側で完結
* タグベースの状態管理：unread/カスタムタグ
* 設定ファイル（YAML）からのフィード管理
* CLI Mode（都度実行）を基本とする（RPCは将来拡張として扱う）

### 非目的（当面）

* フィードのCRUD操作（設定ファイルを直接編集）
* 古いエントリの自動削除（SQLiteを直接操作）
* 多クライアントの厳密同期
* リモート公開前提の認証・権限管理

## A2. 実行形態

### A2.1 CLI Mode（デフォルト）

* `feeder <command>` を都度起動し、主要な結果は **標準出力（stdout）** に出すのだ
  - 出力形式は `--output json|plain`（または設定）で切り替えるのだ
* `--output json`（機械可読）
  - 成功：exit code 0 + JSON（共通envelope）
  - 致命：exit code != 0 + JSON（共通envelopeの `error`）
  - デバッグ/トレース等の詳細は **標準エラー（stderr）** に寄せるのだ（通常は出さない）
* `--output plain`（対話向け）
  - 成功/失敗ともに、人間向け表示を stdout/stderr に出してよいのだ（契約は緩め）
  - `--help` は plain 前提でよい（機械可読契約の対象外）なのだ

**コマンド一覧：**

```
feeder sync                      # 同期実行
feeder list [--query <q>]        # エントリ一覧
feeder view <id>                 # エントリ詳細
feeder mark <operation> <ids>    # 状態更新
feeder tags                      # タグ一覧
feeder feeds                     # フィード一覧
feeder feeds --config-check      # 設定ファイルの静的妥当性検証（DB非依存）

```

**共通フラグ：**

```
--config <path>     # config.toml のパス（デフォルト: ~/.config/feeder/config.toml）
--db <path>         # DB パスの上書き（テスト用）
--output <json|plain> # 出力形式（デフォルト: plain）
--debug             # デバッグ情報をstderrに出す（json出力を壊さない）
--trace             # より詳細なトレースをstderrに出す（必要ならbacktraceも）

```

### A2.2 RPC Mode（オプション）

* `feeder serve` でstdio JSON-RPC 2.0サーバー起動
* 双方向通信、進捗通知（notification）対応

**メソッド一覧：**

* `sync.run` → 進捗通知: `sync.progress`
* `entries.list`
* `entries.get`
* `entries.mark`
* `feeds.list`
* `tags.list`

**Notification例：**

* `sync.progress(current, total, feed)`
* `entries.updated(entry_ids)`
* `log(level, message)`
