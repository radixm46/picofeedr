# 日付検索仕様（現行）

## 1. 目的

本ドキュメントは、`list --query` における日付検索の現行仕様を定義する。
絶対日付と相対durationの両方を受理し、同一クエリ内で一貫した評価結果を返す。

## 2. 現行仕様（前提）

- `after:` は下限（inclusive）
- `before:` は上限（exclusive）
- `date = COALESCE(published_at, updated_at, first_seen_at)` に適用
- `after` と `before` を併用する場合は `after < before` を必須

## 3. 設計原則

- 同一意味の条件は最終的に「下限/上限」の2軸へ正規化する
- 下限と上限は優先順位で勝ち負けを付けず、交差（AND）として合成する
- 相対時刻は評価時点 (`now`) を固定して計算する
- 無効レンジは静かに0件にせず `INVALID_QUERY` を返す

## 4. 受理構文

- `after:` / `before:` の値として、絶対日付と相対 duration を受理する
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

## 5. 正規化仕様

最終評価は以下に正規化する。

- `after:` は実効下限 `effective_after` を与える
- `before:` は実効上限 `effective_before` を与える
- `after:` / `before:` はそれぞれ 1 回のみ指定でき、重複指定は `INVALID_QUERY`（`doc/spec/query.md` の Top-Level Rules を正本とする）
- 妥当性: 両方を指定した場合は `effective_after < effective_before` を必須とする

### 5.1 変換規則

- `after:YYYY-MM-DD` -> `effective_after = 当該ローカル日付 0:00`
- `before:YYYY-MM-DD` -> `effective_before = 当該ローカル日付 0:00`
- `after:DUR` -> `effective_after = now - DUR`
- `before:DUR` -> `effective_before = now - DUR`

### 5.2 併用例

- `after:2026-01-01 before:2w`
  - 絶対日付と相対 duration の混在を許可する
- `after:3m before:1y`
  - 状況によっては `effective_after >= effective_before` となり `INVALID_QUERY`

## 6. `now` と再現性

- `now` はクエリ評価開始時の1回だけ取得し、同一リクエスト内で固定する
- ページング継続時の再現性のため、カーソル検証に使う `query_hash` には正規化後の境界値が反映される

### 6.1 タイムゾーン方針（決定）

- ロケールではなく **タイムゾーン（TZ）** を基準に扱う
- 絶対日付と相対時刻のどちらも `system default timezone` で日付境界を決定する
- 相対時刻（`after:3m` / `before:14d` など）の計算は `system default timezone` で行う
- 計算結果は内部で UTC epoch に正規化して評価する
- 1クエリ中は評価時刻とタイムゾーンを固定値として扱う

### 6.2 実装メモ

- DST（月/日境界）による揺れを避けるため、先に TZ でカレンダー演算し、その結果を UTC へ変換する
- cursor 再評価時の一貫性担保のため、解決後の境界エポック（`after` / `before`）が `query_hash` に反映される

## 7. エラー仕様（追加）

- 不正 duration: `INVALID_QUERY`（例: `after:3x`）とする
- 負数 duration: `INVALID_QUERY` とする
- 0 duration: 許可する
- 無効レンジ（`after >= before`）: `INVALID_QUERY` とする

## 8. 補足

- 相対値を `after/before` に統一することで、CLIと保存済みクエリで同じ記法を共有できる。
- 詳細な全体クエリ文法は `doc/spec/query.md` を正本とする。
