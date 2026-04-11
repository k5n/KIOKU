# Phase 5.6 実施計画

## 1. 目的

Phase 5.6 の目的は、`crates/evaluate` に残っている  
**raw retrieval item 配列前提の I/F と artifact schema を削除し、LoCoMo / LongMemEval の KIOKU 評価 path を `prompt_context` 中心へ完全に寄せること** です。

現状の LoCoMo / LongMemEval runner はどちらも、

- backend から `QueryOutput` を受け取る
- retrieval judge は `prompt_context.text` を採点する
- answerer も同じ `prompt_context.text` を使う

という構成になっています。  
この意味論では、`QueryOutput.retrieved` は answer semantics / retrieval semantics のどちらにも直接使われておらず、  
残っているのは主に次の二次利用だけです。

- `retrieval.jsonl` の `retrieved_count`
- `retrieval.jsonl` の `retrieved_memory_ids`
- `retrieval.jsonl` の `retrieved_source_event_ids`
- metrics の `average_retrieved_item_count`
- prompt builder / backend test の補助引数

したがって Phase 5.6 では、`QueryOutput.retrieved` と `RetrievedMemory` を軸にした設計を撤去し、  
**評価系の正規 contract は `prompt_context` と backend metadata のみ** に整理します。

## 2. 前提整理

### 2.1 現状コードの観察

現状の主要参照は次です。

1. `crates/evaluate/src/model/retrieval.rs`
   - `QueryOutput.retrieved`
   - `RetrievedMemory`
2. `crates/evaluate/src/prompt/answer.rs`
   - `PromptBuildRequest.retrieved`
   - test 用の `sample_retrieved()`
3. `crates/evaluate/src/runner/locomo_kioku.rs`
   - `RetrievalLogRecord` の `retrieved_*`
   - metrics input の `retrieved_count`
4. `crates/evaluate/src/runner/longmemeval_kioku.rs`
   - `RetrievalLogRecord` の `retrieved_*`
   - metrics input の `retrieved_count`
5. `crates/evaluate/src/backend/return_all.rs`
   - prompt context 生成のために `retrieved` を経由している LoCoMo path
   - query test が `output.retrieved` を検証している
6. `crates/evaluate/src/model/metrics.rs`
   - `RetrievalLogRecord.retrieved_count`
   - `RetrievalLogRecord.retrieved_memory_ids`
   - `RetrievalLogRecord.retrieved_source_event_ids`
   - `DatasetMetrics.average_retrieved_item_count`
7. `doc/KIOKU-LoCoMo-evaludation.md`
   - retrieval artifact schema が raw item 追跡を前提にしている
8. `doc/KIOKU-LongMemEval-evaludation.md`
   - retrieval artifact schema が raw item 追跡を前提にしている

### 2.2 現状の問題

この状態には次の問題があります。

1. runner の意味論は `prompt_context` 中心なのに、model と artifact schema だけが raw item 配列を正規入力として引きずっている
2. `ReturnAllMemoryBackend` が LoCoMo context を作るためだけに一度 `RetrievedMemory` へ変換しており、不要な中間表現が残っている
3. `average_retrieved_item_count` は judge / answerer が見ていない値であり、主要評価指標に混ざると解釈がぶれる
4. `retrieved_memory_ids` / `retrieved_source_event_ids` は汎用 backend 契約を重くする一方、現在の評価仕様では必須ではない
5. `PromptBuildRequest.retrieved` は LoCoMo / LongMemEval の両 prompt builder で実質未使用であり、型だけが古い設計を示している

## 3. Phase 5.6 の方針

### 3.1 `QueryOutput` の正規出力を `prompt_context` + `metadata` に絞る

Phase 5.6 後の `QueryOutput` は次の方向に整理します。

```rust
pub struct QueryOutput {
    pub prompt_context: PromptContext,
    pub metadata: Value,
}
```

`crates/evaluate` の `MemoryBackend` trait は評価 protocol 用の I/F であり、  
LoCoMo / LongMemEval の KIOKU 評価 path では `prompt_context` が常に必須です。  
したがって `QueryOutput.prompt_context` は `Option` にせず、  
**backend adapter 側で必ず evaluation-ready な `PromptContext` へ正規化して返す** 方針を採ります。

### 3.2 `RetrievedMemory` は評価の正規 model から削除する

`RetrievedMemory` は `QueryOutput.retrieved` のためだけに存在しており、  
現状コードでは評価の主要 path から独立した value object になっていません。

そのため Phase 5.6 では、

- `crates/evaluate` の public model export から `RetrievedMemory` を削除する
- prompt builder の request からも削除する
- backend trait は raw item 配列を返さない contract にする

方針を採ります。

将来 raw retrieval item の観測が必要になった場合は、  
`QueryOutput.metadata` 配下の dataset-specific / backend-specific diagnostics として戻す方が自然です。  
少なくとも LoCoMo / LongMemEval の main evaluation contract には戻しません。

### 3.3 raw retrieval item 指標は標準 artifact から外す

`retrieved_count` 系の値は、`retrieved` を削除する以上そのまま維持できません。  
ここでは無理に代替 field を作らず、**標準 artifact schema から削除する** 方針を採ります。

削除対象:

- `RetrievalLogRecord.retrieved_count`
- `RetrievalLogRecord.retrieved_memory_ids`
- `RetrievalLogRecord.retrieved_source_event_ids`
- `DatasetMetrics.average_retrieved_item_count`
- runner metrics input にある `retrieved_count`

この判断の理由は次です。

1. これらは `prompt_context` 中心の評価仕様における主要観測量ではない
2. backend ごとに raw retrieval item の意味論が揺れやすい
3. 不完全な抽象を維持するより、artifact を小さく正確にする方が保守しやすい

### 3.4 backend 診断が必要なら `metadata` に閉じ込める

raw retrieval の件数や event ID を将来見たくなる可能性はあります。  
ただしそれは評価 protocol の必須 schema ではなく、backend 診断情報です。

したがって必要なら次の方向だけ許容します。

- `QueryOutput.metadata`
- `RetrievalLogRecord.metadata`
- backend 固有の debug output

Phase 5.6 では共通 schema に `raw_retrieval_*` のような新 field は追加しません。  
まずは削除して contract を軽くすることを優先します。

## 4. 完了条件

Phase 5.6 の完了条件は次です。

1. `QueryOutput.retrieved` が削除されている
2. `RetrievedMemory` が `crates/evaluate` の model から削除されている
3. `PromptBuildRequest.retrieved` が削除されている
4. `MemoryBackend::query` の実装群が raw retrieval item を返さなくなっている
5. `QueryOutput.prompt_context` が必須 field になっている
6. LoCoMo / LongMemEval runner が `prompt_context` のみで動作する
7. runner から `missing prompt_context` 用の fail-fast が削除されている
8. `RetrievalLogRecord` から `retrieved_count` / `retrieved_memory_ids` / `retrieved_source_event_ids` が削除されている
9. `DatasetMetrics.average_retrieved_item_count` が削除されている
10. `runner/metrics.rs` の入力 struct から `retrieved_count` が削除されている
11. `return-all` backend が `RetrievedMemory` を経由せずに deterministic な `prompt_context` を返す
12. LoCoMo / LongMemEval の end-to-end test が新 schema で通る
13. `runner/output.rs` の artifact schema test が新 field set に更新されている
14. LoCoMo / LongMemEval の仕様文書から raw retrieval item 前提の記述が整理されている

## 5. 影響範囲

### 5.1 model / trait

対象:

- `crates/evaluate/src/model/retrieval.rs`
- `crates/evaluate/src/model/mod.rs`
- `crates/evaluate/src/backend/traits.rs`

やること:

1. `QueryOutput` から `retrieved` を削除する
2. `QueryOutput.prompt_context` を必須 field にする
3. `RetrievedMemory` struct を削除する
4. `pub use retrieval::{..., RetrievedMemory};` を削除する
5. backend trait 実装が新 `QueryOutput` を返すよう更新する

### 5.2 prompt

対象:

- `crates/evaluate/src/prompt/answer.rs`

やること:

1. `RetrievedMemory` import を削除する
2. `PromptBuildRequest.retrieved` を削除する
3. unit test の `sample_retrieved()` を削除する
4. prompt builder test を `prompt_context` のみを渡す形に更新する

### 5.3 runner

対象:

- `crates/evaluate/src/runner/locomo_kioku.rs`
- `crates/evaluate/src/runner/longmemeval_kioku.rs`

やること:

1. `build_answer_prompt` 呼び出しから `retrieved` を外す
2. `RetrievalLogRecord` 構築時の `retrieved_*` フィールド参照を削除する
3. metrics input の `retrieved_count` を削除する
4. `missing prompt_context` 前提の fail-fast を削除する
5. `MissingPromptContextBackend` test fixture を削除または通常 backend fixture に置換する

### 5.4 backend

対象:

- `crates/evaluate/src/backend/return_all.rs`

やること:

1. `selected_events` から直接 `PromptContext` を構築する
2. LoCoMo 用 context renderer を `BenchmarkEvent` ベースへ切り替える
3. `QueryOutput` は `prompt_context` と `metadata` のみ返す
4. backend test の期待値を `output.retrieved` 依存から外す

### 5.5 metrics / outputs

対象:

- `crates/evaluate/src/model/metrics.rs`
- `crates/evaluate/src/runner/metrics.rs`
- `crates/evaluate/src/runner/output.rs`

やること:

1. `RetrievalLogRecord` から `retrieved_*` を削除する
2. `DatasetMetrics.average_retrieved_item_count` を削除する
3. LoCoMo / LongMemEval metrics builder から平均 retrieval 件数集計を削除する
4. JSON output test fixture を新 schema に揃える

### 5.6 docs

対象:

- `doc/KIOKU-LoCoMo-evaludation.md`
- `doc/KIOKU-LongMemEval-evaludation.md`
- `doc/implement-evaluation-plan.md`
- 必要に応じて `doc/evaluation.md`

やること:

1. retrieval artifact schema から `retrieved_*` を削除する
2. metrics schema から `average_retrieved_item_count` を削除する
3. `prompt_context.text` が唯一の retrieval 評価対象であることを再度明示する
4. raw retrieval diagnostics が必要なら metadata 扱いであることを記述する
5. `implement-evaluation-plan.md` に Phase 5.6 の contract 変更のみを補足追記する

## 6. 実装手順

### Step 1. `QueryOutput` と `PromptBuildRequest` から `retrieved` を外す

最初に API の中心から不要 field を外します。

実施内容:

1. `model/retrieval.rs` から `QueryOutput.retrieved` と `RetrievedMemory` を削除する
2. `QueryOutput.prompt_context` を必須 field に変更する
3. `model/mod.rs` の re-export を更新する
4. `prompt/answer.rs` から `PromptBuildRequest.retrieved` を削除する
5. prompt builder test を新 API に合わせる

この Step の目的は、  
**compile error を使って `retrieved` 依存箇所を機械的に洗い出せる状態を作ること** です。

### Step 2. runner と test fixture の `retrieved` 依存を除去する

次に main pipeline を `prompt_context` 専用 path に揃えます。

実施内容:

1. LoCoMo runner の `build_answer_prompt` 呼び出しを更新する
2. LongMemEval runner の `build_answer_prompt` 呼び出しを更新する
3. runner 側の `missing prompt_context` fail-fast を削除する
4. `MissingPromptContextBackend` fixture を削除または通常 backend fixture に置換する
5. `retrieval_judge_and_answerer_share_same_context_text` 系 test が `prompt_context` だけで成立することを確認する

### Step 3. `RetrievalLogRecord` と metrics から raw retrieval 集計を削除する

次に artifact schema を整理します。

実施内容:

1. `RetrievalLogRecord` から `retrieved_count` / `retrieved_memory_ids` / `retrieved_source_event_ids` を削除する
2. `runner/metrics.rs` の入力 struct から `retrieved_count` を削除する
3. `DatasetMetrics.average_retrieved_item_count` を削除する
4. metrics builder test の期待値を更新する
5. `runner/output.rs` の schema test を更新する

この Step の目的は、  
**model 削除に伴う artifact のねじれをなくすこと** です。

### Step 4. `ReturnAllMemoryBackend` を event 直結にする

LoCoMo の context 生成で残っている中間変換を外します。

実施内容:

1. `render_locomo_context` を `&[BenchmarkEvent]` ベースへ変更する
2. `build_prompt_context` が `RetrievedMemory` を受け取らない形にする
3. LongMemEval / LoCoMo backend test を `prompt_context` 中心に更新する

この Step の目的は、  
**stub backend からも raw retrieval abstraction を完全に除去すること** です。

### Step 5. 文書と schema expectation を更新する

最後に仕様文書と test fixture を実装に合わせます。

実施内容:

1. LoCoMo / LongMemEval 評価仕様文書の artifact 例を更新する
2. `average_retrieved_item_count` の説明を削除する
3. retrieval artifact の説明を `context_kind` / `context_text` / judge metadata 中心へ書き換える

## 7. 判断が必要な点

### 7.1 `prompt_context` は必須 field にする

`crates/evaluate` の `MemoryBackend` trait は汎用検索 API ではなく、  
LoCoMo / LongMemEval を同一評価仕様で比較するための evaluation protocol です。  
そのため `QueryOutput.prompt_context` は `Option` にせず、backend adapter 側で必ず  
evaluation-ready な `PromptContext` に正規化して返す方針を採ります。

この判断により、

1. runner 側の `missing prompt_context` fail-fast が不要になる
2. `prompt_context` 必須という評価仕様を型で表現できる
3. 将来 KIOKU 以外の backend を比較する場合も、同一 protocol への正規化責務を adapter 側へ固定できる

という利点があります。

### 7.2 raw retrieval diagnostics を metadata に即座に載せるか

今回は **載せない** 方針を基本とします。  
理由は、削除対象を減らすのではなく contract を軽くすることが主目的だからです。  
必要性が出た時点で backend-specific metadata として追加すれば十分です。

### 7.3 `average_retrieved_item_count` の削除影響

これは metrics schema の破壊的変更です。  
ただし Phase 5 以降の意味論では `prompt_context` の token 数の方が比較上重要であり、  
raw retrieval 件数を main schema に残す価値は低いと判断します。

## 8. 検証方針

最低限の確認は次です。

1. `cargo test -p evaluate` が通る
2. LoCoMo runner が `prompt_context` 必須前提で通る
3. LongMemEval runner が `prompt_context` 必須前提で通る
4. retrieval judge と answerer が同じ `prompt_context.text` を使う test が通る
5. `runner/output.rs` の JSON schema test が通る
6. `return-all` backend test が `prompt_context` のみで成立する
7. `MissingPromptContextBackend` のような旧 optional contract 前提 fixture / test が削除されている

## 9. この Phase でやらないこと

次は Phase 5.6 のスコープ外とします。

- backend 固有の debug artifact 追加
- raw retrieval 件数の代替指標新設
- `prompt_context` 自体の structure 拡張
- LoCoMo / LongMemEval 以外の dataset 追加
- `KiokuMemoryBackend` 本実装

## 10. 期待する着地

Phase 5.6 完了後、`crates/evaluate` の retrieval contract は次のように読める状態になります。

- backend は evaluation-ready な `prompt_context` を返す
- runner はその `prompt_context.text` を retrieval judge と answerer に共有する
- artifact はその共有された context と judge 結果だけを正規記録する

つまり、`crates/evaluate` は  
**「何件の raw item を取ったか」を中心にした評価基盤ではなく、  
「answerer に実際に渡した文脈が十分だったか」を中心にした評価基盤** へ揃います。
