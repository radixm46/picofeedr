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

## 4. 採用方針（案B'）

- `after:` / `before:` の値として、絶対日付と相対 duration を許可する
- 新キーワード（`newer_than` / `older_than`）は追加しない
- date math 形式（`now-3m` など）は導入しない

### 4.1 記法

- 絶対日付: `YYYY-MM-DD`
- 相対 duration: `N[d|w|m|y]`
  - `d`: days
  - `w`: weeks
  - `m`: months
  - `y`: years
- `m` は months を表す。minutes は非対応
- 絶対日付 (`YYYY-MM-DD`) もローカル日付 0:00 として解決する
- 境界はローカル日付 0:00 基準で計算するため、`0d` / `0w` / `0m` / `0y` は同義

### 4.2 受理例

- 例:
  - `after:2026-01-01`
  - `before:2026-02-01`
  - `after:2w`
  - `after:3m`
  - `before:14d`
  - `after:3m before:2026-01-01`
  - `after:2026-01-01 before:3m`

## 5. 正規化仕様（必須）

最終評価は以下に正規化する。

- `lower_bounds`（下限候補）と `upper_bounds`（上限候補）を収集
- 実効下限: `effective_after = max(lower_bounds)`
- 実効上限: `effective_before = min(upper_bounds)`
- 妥当性: `effective_after < effective_before` を必須

### 5.1 変換規則

- `after:YYYY-MM-DD` -> lower bound 追加
- `before:YYYY-MM-DD` -> upper bound 追加
- `after:DUR` -> lower bound 追加（`now - DUR`）
- `before:DUR` -> upper bound 追加（`now - DUR`）

### 5.2 併用例

- `after:3m after:2026-01-01`
  - `effective_after = max(now-3m, 2026-01-01)`
- `before:1y before:2026-01-03`
  - `effective_before = min(now-1y, 2026-01-03)`
- `after:3m before:1y`
  - 状況によっては `effective_after >= effective_before` となり `INVALID_QUERY`

## 6. `now` と再現性

- `now` はクエリ評価開始時の1回だけ取得し、同一リクエスト内で固定する
- ページング（cursor）継続時の再現性を担保するため、`query_hash` には次を含める
  - 正規化後の `effective_after` / `effective_before`
  - または `as_of_epoch`（評価時刻）

### 6.1 タイムゾーン方針（決定）

- ロケールではなく **タイムゾーン（TZ）** を基準に扱う
- 絶対日付と相対時刻のどちらも `system default timezone` で日付境界を決定する
- 相対時刻（`after:3m` / `before:14d` など）の計算は `system default timezone` で行う
- 計算結果は内部で UTC epoch に正規化して評価する
- 1クエリ中は `as_of_epoch_utc` と `timezone` を固定値として扱う

### 6.2 実装メモ

- DST（月/日境界）による揺れを避けるため、先に TZ でカレンダー演算し、その結果を UTC へ変換する
- cursor 再評価時の一貫性担保のため、`query_hash` には `as_of_epoch_utc` と `timezone` を含める

## 7. エラー仕様（追加）

- 不正 duration: `INVALID_QUERY`（例: `after:3x`）
- 負数 duration: `INVALID_QUERY`
- 0 duration: 許可するか禁止するかを明示（初期は許可推奨）
- 無効レンジ（`after >= before`）: `INVALID_QUERY`

## 8. 導入ステップ（提案）

### Phase 1

- `after/before` の値として `YYYY-MM-DD` と `N[d|w|m|y]` を実装
- 正規化基盤（max/min + 妥当性チェック）を実装
- 混在指定（絶対 + 相対）を受理

### Phase 2

- 追加の duration 記法（必要なら）を検討
- 既存正規化基盤を再利用

## 9. コスト見積もり（実装目安）

- Phase 1（`after/before` への duration 導入 + 正規化 + テスト）: 中
- Phase 2（追加記法導入 + テスト追加）: 小〜中

備考:

- 相対値を `after/before` に統一すると、UI/CLI で同一記法を共有しやすい。
