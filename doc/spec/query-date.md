# 日付検索拡張仕様（Draft）

## 1. 目的

本ドキュメントは、日付検索の拡張構文を導入する際の仕様方針を定義する。
現行の `after:YYYY-MM-DD` / `before:YYYY-MM-DD` に対して、相対時間指定（例: 3ヶ月前）を安全に導入し、
CLI / UI / 保存クエリで一貫した挙動を提供することを目的とする。

## 2. 現行仕様（前提）

- `after:` は下限（inclusive）
- `before:` は上限（exclusive）
- `date = COALESCE(published_at, updated_at, first_seen_at)` に適用
- `after` と `before` を併用する場合は `after < before` を必須

## 3. 設計原則

- 同一意味の条件は最終的に「下限/上限」の2軸へ正規化する
- 複数条件は優先順位で勝ち負けを付けず、交差（AND）として合成する
- 相対時刻は評価時点 (`now`) を固定して計算する
- 無効レンジは静かに0件にせず `INVALID_QUERY` を返す

## 4. 導入案

### 4.1 案A: `newer_than` / `older_than` を追加（推奨初期案）

- 追加構文:
  - `newer_than:<duration>` （下限）
  - `older_than:<duration>` （上限）
- 期間記法（案）: `Nd`, `Nw`, `Nm`, `Ny`（例: `3m`, `14d`）
- 例:
  - `newer_than:3m`
  - `older_than:1y`
  - `newer_than:3m before:2026-01-03`

### 4.2 案B: `after:` / `before:` の値として date math を許可

- 例:
  - `after:now-3m`
  - `before:now-7d`
  - `after:now-3m before:2026-01-03`
- 構文種別を増やさない利点はあるが、字句/構文解析とエラー表現の複雑度が上がる

### 4.3 案C: UI 側変換のみ（非推奨）

- UI で「3ヶ月前まで」を絶対日付へ変換して `after/before` に落とし込む
- CLI と挙動が分岐し、仕様の単一性を損なうため非推奨

## 5. 正規化仕様（必須）

どの案でも、最終評価は以下に正規化する。

- `lower_bounds`（下限候補）と `upper_bounds`（上限候補）を収集
- 実効下限: `effective_after = max(lower_bounds)`
- 実効上限: `effective_before = min(upper_bounds)`
- 妥当性: `effective_after < effective_before` を必須

### 5.1 変換規則

- `after:DATE` -> lower bound 追加
- `before:DATE` -> upper bound 追加
- `newer_than:DUR` -> lower bound 追加（`now - DUR`）
- `older_than:DUR` -> upper bound 追加（`now - DUR`）

### 5.2 併用例

- `newer_than:3m after:2026-01-01`
  - `effective_after = max(now-3m, 2026-01-01)`
- `older_than:1y before:2026-01-03`
  - `effective_before = min(now-1y, 2026-01-03)`
- `newer_than:3m older_than:1y`
  - 状況によっては `effective_after >= effective_before` となり `INVALID_QUERY`

## 6. `now` と再現性

- `now` はクエリ評価開始時の1回だけ取得し、同一リクエスト内で固定する
- ページング（cursor）継続時の再現性を担保するため、`query_hash` には次を含める
  - 正規化後の `effective_after` / `effective_before`
  - または `as_of_epoch`（評価時刻）

### 6.1 タイムゾーン方針（決定）

- ロケールではなく **タイムゾーン（TZ）** を基準に扱う
- 相対時刻（`now-3m` / `newer_than:3m` など）の計算は `system default timezone` で行う
- 計算結果は内部で UTC epoch に正規化して評価する
- 1クエリ中は `as_of_epoch_utc` と `timezone` を固定値として扱う

### 6.2 実装メモ

- DST（月/日境界）による揺れを避けるため、先に TZ でカレンダー演算し、その結果を UTC へ変換する
- cursor 再評価時の一貫性担保のため、`query_hash` には `as_of_epoch_utc` と `timezone` を含める

## 7. エラー仕様（追加）

- 不正 duration: `INVALID_QUERY`（例: `newer_than:3x`）
- 負数 duration: `INVALID_QUERY`
- 0 duration: 許可するか禁止するかを明示（初期は許可推奨）
- 無効レンジ（`after >= before`）: `INVALID_QUERY`

## 8. 導入ステップ（提案）

### Phase 1

- `newer_than` / `older_than` を追加
- 正規化基盤（max/min + 妥当性チェック）を実装
- `after/before` は絶対日付のみ継続

### Phase 2

- 必要性を確認して `after/before` の date-math (`now-...`) を導入
- 既存正規化基盤を再利用

## 9. コスト見積もり（実装目安）

- Phase 1（`newer_than` / `older_than` + 正規化 + テスト）: 中
- Phase 2（`after:now-...` / `before:now-...` 追加）: 中〜大

備考:

- 先に Phase 1 を入れると、Phase 2 は正規化ロジックを流用できるため追加コストを圧縮しやすい。
