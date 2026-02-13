# B. UI / クライアント設計ノート（非規約）

## B0. 役割分担（クライアント側）

- 画面：一覧表示、フィルタ入力、選択、本文表示
- 操作：既読/スター/タグ付与・除去
- ページング：`next_cursor` の保持と次ページ要求
- UI都合の状態（選択中のID、表示順、ローカル索引）はクライアントが保持

## B1. クライアントキャッシュ（推奨）

### B1.1 feed index キャッシュ

- `feeds` コマンドの結果をローカル保存
- 次回起動時に差分確認（変更があれば再取得）

### B1.2 tag index キャッシュ（任意）

- `tags` コマンドの結果を同様に保存

## B2. ページングのUI運用（推奨）

- `total_hits` は変動しうるため参考値として扱う
- `sort` 変更時は cursor を破棄して再検索
- 大量ヒット時でもUIが重くならないよう、既定 `limit=100` を守り、追加ロードで増やす
- `next_cursor` は完全不透明として扱い、内部JSONを解釈せずに保持して再送する
- `revision` / `updated_at` は表示・診断用メタであり、ページ継続キーとしては使わない

## B3. "読んだら消える"運用の想定

- `unread` 一覧を読み進めて既読化する場合、 続き取得は「最後に受け取った `next_cursor`」を使う
- クライアントは表示上 `unread` を外したアイテムを即時反映してよい

## B4. バックエンドモードの選択

### B4.1 CLI Mode（推奨デフォルト）

**使用ケース：**

- 120 feeds以下
- 手動sync中心
- シンプルな実装を優先

**性能：**

- プロセス起動オーバーヘッド：約10ms/回（Go/Rust）
- syncはユーザーが明示的に叩く前提（常時ポーリング前提にしない）

### B4.2 RPC Mode（パワーユーザー向け）

**使用ケース：**

- 真のリアルタイム進捗が欲しい
- entry単位の通知が必要
- 複雑なワークフロー

**利点：**

- プロセス起動なし
- 即座の進捗通知
- 双方向通信

## B5. Emacsクライアント実装例（CLI Mode）

```

(defun picofeedr-sync () "Run sync synchronously and refresh list." (interactive) (let* ((resp (picofeedr-cli-json "sync")) (data (alist-get 'data resp))) (message "Sync completed: fetched=%s new_entries=%s" (alist-get 'fetched data) (alist-get 'new_entries data)) (picofeedr-refresh-list)))

(defun picofeedr-cli-json (&rest args) "Run picofeedr command and parse JSON output." (with-temp-buffer (apply #'call-process "picofeedr" nil t nil "--output" "json" args) (goto-char (point-min)) (json-parse-buffer :object-type 'alist)))

(defun picofeedr-list () "Show entry list." (interactive) (let* ((resp (picofeedr-cli-json "list" "--query" "unread" "--limit" "100")) (data (alist-get 'data resp))) ;; Display items... ))

(defun picofeedr-mark-read (&rest ids) "Mark entries as read." (apply #'picofeedr-cli-json "mark" "read" (mapcar #'number-to-string ids)))

```

---
