# Phase 5 実装計画

## 1. 目的

Phase 5 の目的は、`crates/evaluate` の LongMemEval 実行パスを暫定 exact match judge から切り替え、  
**KIOKU 用評価仕様 `longmemeval_kioku_v1`** に沿って answer correctness と gold-conditioned retrieval sufficiency を同一 protocol で評価できる状態にすることです。

この Phase で先に完成させるのは、次の 5 点です。

1. LongMemEval の answer semantics を `longmemeval_kioku_v1` に固定する
2. LongMemEval の retrieval semantics を `prompt_context.text` 基準の gold-conditioned retrieval sufficiency に置き換える
3. `answers.jsonl` / `retrieval.jsonl` / `metrics.json` / `run.resolved.json` の意味論を LongMemEval 用 protocol に揃える
4. abstention を main backend score から分離したまま記録・集計できるようにする
5. 将来の `KiokuMemoryBackend` が `prompt_context` を返せばそのまま接続できる runner / judge / metrics の形を固める

Phase 4 が LoCoMo の protocol 移行だったのに対し、Phase 5 は **LongMemEval を `longmemeval_kioku_v1` へ移行する Phase** です。  
ここで official leaderboard の忠実再現を目指すのではなく、KIOKU backend 間比較に必要な再現性と解釈性を優先します。

## 2. Phase 5 の完了条件

Phase 5 の完了条件は次です。

1. LongMemEval 実行時の judge semantics が `LongMemEvalJudge` の暫定 exact match ではなく `longmemeval_kioku_v1` になる
2. LongMemEval 実行時に `AnswerJudge` と `RetrievalJudge` の 2 系統 judge が使われる
3. retrieval judge が `question + gold answers + question_type + question_date + is_abstention + prompt_context.text` を入力に `SUFFICIENT / INSUFFICIENT` を返せる
4. answer judge が `question + gold answers + question_type + question_date + is_abstention + generated answer` を入力に `CORRECT / WRONG` を返せる
5. LongMemEval + `longmemeval_kioku_v1` では `prompt_context` が必須になり、欠落時は fail-fast で error になる
6. retrieval judge と answerer が同じ `prompt_context.text` を参照する実行順序に整理されている
7. LongMemEval 用 answer prompt が仕様書の system / user prompt と `NOT_ENOUGH_MEMORY` sentinel に固定される
8. `metrics.json` が `overall_answer_accuracy`、`task_averaged_answer_accuracy`、`abstention_answer_accuracy`、`overall_retrieval_sufficiency_accuracy`、`task_averaged_retrieval_sufficiency_accuracy` を出せる
9. per-type answer / retrieval accuracy が non-abstention のみを分母として集計される
10. `question_count`、`non_abstention_question_count`、`abstention_question_count`、`average_context_token_count` を出せる
11. `answers.jsonl` と `retrieval.jsonl` が LongMemEval 用 judge metadata を保持する
12. `run.resolved.json` に LongMemEval 用 prompt と judge の provenance が残る
13. `return-all` もしくは mock backend で `longmemeval_kioku_v1` の end-to-end test が通る
14. LoCoMo の `locomo_kioku_v1` path は Phase 5 の変更で壊れない

## 3. 前提整理

### 3.1 Phase 4 まででできていること

Phase 4 までで、LongMemEval に必要な下地はかなり揃っています。

- LongMemEval loader / adapter は `has_answer` を保持し、canonical ID へ正規化できる
- `PromptContextKind` と `QueryOutput.prompt_context` がある
- `AnswerJudge` / `RetrievalJudge` trait は導入済み
- judge 用 OpenAI 互換 runtime と judge 設定は共通化済み
- LoCoMo には dual-judge 専用 pipeline がある
- `RetrievalLogRecord` と `MetricsReport` は dual-judge semantics を表現できる

つまり、Phase 5 の主作業は基盤新設ではなく、**LongMemEval path を暫定 pipeline から protocol-specific pipeline へ移すこと** です。

### 3.2 現状コードと `longmemeval_kioku_v1` の主なギャップ

現状の `crates/evaluate` を基準に見ると、ギャップは次です。

1. CLI の LongMemEval path はまだ `EvaluatePipeline + LongMemEvalJudge` の単一 judge 構成である
2. `LongMemEvalJudge` は exact match + 簡易 abstention marker 判定であり、question type rubric を持たない
3. LongMemEval 実行時の `prompt_context` は optional のままで、`retrieved` から fallback 合成できてしまう
4. LongMemEval answer prompt は Phase 3 の profile 切替前提で、`longmemeval_kioku_v1` の固定 prompt ではない
5. retrieval judge が未実装なので、`retrieval.jsonl` の `is_sufficient` / `score` / `label` が埋まらない
6. metrics builder は LoCoMo 用 dual-judge 集計しか持たず、LongMemEval 用 overall / task-averaged / abstention 分離集計がない
7. `average_context_token_count` を計算する tokenizer I/F がない
8. LongMemEval 用 judge prompt ID や answer prompt ID を設定・出力へ明示する形がまだない

### 3.3 Phase 5 で意図的にやらないこと

次は Phase 5 のスコープ外とします。

- LongMemEval official retrieval recall / nDCG の headline 化
- official leaderboard 数値との厳密比較
- judge の 3 回実行や多数決
- human annotation による judge 校正
- retrieved facts の faithfulness judge
- `M-cleaned` を main score として扱うこと
- `KiokuMemoryBackend` の本実装
- LoCoMo の semantics 変更

## 4. 設計原則

### 4.1 LongMemEval も dataset-specific pipeline に切り出す

Phase 4 と同じく、LongMemEval も共通 `EvaluatePipeline` に無理に押し込まず、  
**`runner/` 配下の LongMemEval 専用 pipeline** として実装する方が安全です。

理由は次です。

- LongMemEval は answer / retrieval の両方で question type rubric を使う
- abstention を main score から除外しつつ別枠で report する必要がある
- task-averaged accuracy など LoCoMo と異なる集計規則を持つ
- `prompt_context` 必須という強い制約を LongMemEval だけに適用したい

### 4.2 retrieval の評価対象は `prompt_context.text` に固定する

`longmemeval_kioku_v1` では retrieval judge が見るのは `QueryOutput.retrieved` ではなく、  
**Answerer が実際に見た `prompt_context.text`** です。

runner は次の順序を厳密に守ります。

1. backend から `QueryOutput` を受け取る
2. `prompt_context.text` を固定する
3. retrieval judge にその文字列を渡す
4. 同じ文字列から answer prompt を構築する
5. answerer を実行する
6. answer judge を実行する

### 4.3 LongMemEval では fallback を認めない

Phase 3 の prompt profile 実装には `retrieved` からの fallback 合成が残っています。  
しかし `longmemeval_kioku_v1` では、この fallback を許すと retrieval judge と answerer の評価対象が backend ごとにぶれます。

したがって LongMemEval + `longmemeval_kioku_v1` では次を固定します。

- `prompt_context = None` は不正
- `build_answer_prompt` は backend 提供の `prompt_context` を必須にする
- `return-all` backend も LongMemEval 実行時は deterministic な `prompt_context` を返す

### 4.4 answer prompt と judge prompt は分けるが provenance は揃える

LongMemEval では answer prompt と judge prompt の責務が違います。

- answer prompt:
  - answerer に `NOT_ENOUGH_MEMORY` sentinel と参照時刻の解釈を強制する
- retrieval judge prompt:
  - gold-conditioned retrieval sufficiency を採点する
- answer judge prompt:
  - generated answer の correctness を採点する

一方で比較再現性のため、`metrics.json` と `run.resolved.json` には  
**answerer model / answer prompt ID / judge model / judge prompt ID** を一貫して残します。

## 5. 設定と I/F の変更方針

### 5.1 LongMemEval 用 prompt 設定を `kioku_v1` へ寄せる

現状の `[prompt.longmemeval]` は answer profile と `cot` の指定が主です。  
Phase 5 では LongMemEval を `longmemeval_kioku_v1` に切り替えるため、設定の意味論も切り替えます。

必要な方針は次です。

1. LongMemEval 実行時の prompt 設定を `answer_profile` ではなく protocol prompt ID で表現する
2. `answer_template_id = "longmemeval.kioku.answer.v1"` を解決済み設定へ保存する
3. `answer_judge_prompt_id = "longmemeval.kioku.judge.answer.v1"` を保存する
4. `retrieval_judge_prompt_id = "longmemeval.kioku.judge.retrieval.v1"` を保存する

設定表現は 2 通りあります。

- 既存の `[prompt.longmemeval]` を後方互換付きで拡張する
- `[prompt.longmemeval_kioku]` を新設する

Phase 5 では LoCoMo と対称性が高く、validation を分離しやすい  
**`[prompt.longmemeval_kioku]` を新設する案** が有力です。

### 5.2 judge 設定は共通のまま使う

judge transport / retry / temperature / model 設定は LoCoMo で導入済みの共通 `JudgeConfig` を再利用します。  
LongMemEval では次だけを追加で保証すれば十分です。

1. LongMemEval + `longmemeval_kioku_v1` では `[judge]` を必須にする
2. `run.resolved.json` に judge model と prompt ID を残す
3. `metrics.json` の provenance に LongMemEval 用 judge kind を書く

### 5.3 tokenizer I/F を追加する

`average_context_token_count` を出すには、`prompt_context.text` の token 数を run ごとに同じ方法で数える必要があります。

ここでは answerer 実装に依存させず、評価基盤側に薄い tokenizer I/F を追加するのが自然です。

```rust
pub trait TokenCounter: Send + Sync {
    fn count_text_tokens(&self, text: &str) -> anyhow::Result<usize>;
    fn name(&self) -> &'static str;
}
```

Phase 5 の最小実装では次のどちらかで十分です。

1. `WhitespaceTokenCounter` のような暫定実装を入れ、`provisional` を明示する
2. judge / answerer と独立した tokenizer 設定を持たせる

v1 の比較再現性を優先するなら、まずは **固定実装を 1 本に絞り provenance に名称を残す** 方が扱いやすいです。

## 6. prompt の変更方針

### 6.1 Answer prompt を `longmemeval.kioku.answer.v1` に固定する

Phase 3 の LongMemEval prompt profile は、official prompt の変種を切り替える仕組みでした。  
Phase 5 では KIOKU 用 protocol を採るため、answer prompt は次のテンプレートに固定します。

- system prompt:
  - provided memory context のみを使う
  - `question_date` を参照時刻として扱う
  - knowledge-update では最新状態を優先する
  - 外部知識を使わない
  - 不足時は `NOT_ENOUGH_MEMORY` を厳密返却する
  - explanation を出さない
- user prompt:
  - `Memory context`
  - `Current date`
  - `Question`

これにより LongMemEval の answer prompt 分岐はなくなり、  
profile 切替ではなく **backend が返す context の中身そのもの** が比較対象になります。

### 6.2 judge prompt を LongMemEval 用 rubric に固定する

judge prompt は少なくとも 2 本必要です。

1. `longmemeval.kioku.judge.retrieval.v1`
2. `longmemeval.kioku.judge.answer.v1`

どちらの prompt にも次を必ず含めます。

- question
- gold answers
- question type
- question date
- is_abstention
- type-specific rubric

retrieval judge には `prompt_context.text`、answer judge には `generated_answer` を渡します。

### 6.3 rubric 実装は question type 中心にする

LongMemEval の重要点は `question_type` ごとに判定ルールが違うことです。  
そのため prompt renderer 側で type-specific rubric 文面を生成する helper を持たせる方が保守しやすいです。

必要な分岐は次です。

- `single-session-user`
- `single-session-assistant`
- `single-session-preference`
- `temporal-reasoning`
- `knowledge-update`
- `multi-session`
- abstention

abstention は `question_type` ではなく別軸なので、  
type rubric に加えて `is_abstention` に応じた追加指示を重ねる形にします。

## 7. judge の変更方針

### 7.1 `LongMemEvalJudge` を置き換える

現状の `LongMemEvalJudge` は `Judge` trait 実装として残っていますが、  
Phase 5 では LongMemEval path が `Judge` を使わなくなるため、次のいずれかに整理します。

1. `LongMemEvalJudge` を削除し、LongMemEval 用 `AnswerJudge` / `RetrievalJudge` へ置換する
2. 後方互換のため残すが、CLI からは使わない

LoCoMo と揃えるなら、**LongMemEval 用 dual judge を新設し CLI を切り替える** のが自然です。

### 7.2 retrieval judge の出力

retrieval judge の JSON 出力は仕様書どおり次に固定します。

```json
{
  "label": "SUFFICIENT",
  "supported_answer": "blue ceramic mug",
  "reason": "The context contains the user's preference and enough detail to answer the question."
}
```

内部では `BinaryJudgement` に写像します。

- `passed`:
  - `label == "SUFFICIENT"`
- `score`:
  - `1.0` または `0.0`
- `label`:
  - `SUFFICIENT` / `INSUFFICIENT`
- `metadata`:
  - `judge_kind`
  - `judge_model`
  - `judge_prompt_id`
  - `supported_answer`
  - `reason`

### 7.3 answer judge の出力

answer judge の JSON 出力は次に固定します。

```json
{
  "label": "CORRECT",
  "reason": "The generated answer matches the gold answer under the knowledge-update rubric."
}
```

こちらも `BinaryJudgement` に写像します。

- `passed`:
  - `label == "CORRECT"`
- `label`:
  - `CORRECT` / `WRONG`
- `metadata`:
  - `judge_kind`
  - `judge_model`
  - `judge_prompt_id`
  - `reason`

### 7.4 abstention の扱い

Phase 5 の judge 実装では abstention を answer と retrieval で分けて扱います。

- answer judge:
  - abstention も採点対象に含める
  - `NOT_ENOUGH_MEMORY` や意味的同等の差し控えが正しければ `CORRECT`
- retrieval judge:
  - abstention question 自体は採点できても、headline retrieval metrics の分母には入れない

つまり、**judge は全 question を評価できるが、metrics 側で main score の分母を制御する** 形にします。

## 8. runner の変更方針

### 8.1 LongMemEval 専用 pipeline を追加する

`runner/locomo_kioku.rs` と同様に、`runner/longmemeval_kioku.rs` を追加します。  
この pipeline の 1 question フローは次に固定します。

1. backend を case 単位で `reset`
2. event を時系列順に `ingest`
3. backend に `query`
4. `prompt_context` がなければ error
5. retrieval judge を実行
6. `longmemeval.kioku.answer.v1` で回答 prompt を構築
7. answerer を実行
8. answer judge を実行
9. `retrieval.jsonl` と `answers.jsonl` を保存
10. LongMemEval 用 metrics builder に入力を渡す

### 8.2 CLI 分岐を切り替える

`cli/evaluate.rs` の dataset 分岐は次の形に変える必要があります。

- LoCoMo:
  - 既存 `LoCoMoKiokuEvaluatePipeline`
- LongMemEval:
  - 新規 `LongMemEvalKiokuEvaluatePipeline`

これにより `run_with_judge(..., &LongMemEvalJudge, ...)` の path は不要になります。

### 8.3 `return-all` backend の条件を揃える

Phase 5 では LongMemEval でも `prompt_context` 必須なので、  
`ReturnAllMemoryBackend` の LongMemEval path を `longmemeval_kioku_v1` 前提に揃える必要があります。

最低限必要なのは次です。

1. LongMemEval 実行時に deterministic な `PromptContext` を返す
2. 同じ入力なら同じ文字列順になる
3. `question_date` と矛盾しない時系列情報を含められる
4. answerer と retrieval judge が同じ text を参照できる

## 9. logging と metrics の変更方針

### 9.1 `answers.jsonl`

`answers.jsonl` は answer correctness を 1 line = 1 question で保存します。  
LongMemEval 用には少なくとも次を必須で出します。

- `dataset`
- `case_id`
- `question_id`
- `question`
- `question_type`
- `is_abstention`
- `gold_answers`
- `generated_answer`
- `is_correct`
- `score`
- `label`
- `answer_metadata`
- `judgement_metadata`

`answer_metadata` には少なくとも `template_id` と `answerer_model` を残します。

### 9.2 `retrieval.jsonl`

`retrieval.jsonl` は retrieval sufficiency を 1 line = 1 question で保存します。  
LongMemEval 用には次を必須で出します。

- `dataset`
- `case_id`
- `question_id`
- `retrieved_count`
- `retrieved_memory_ids`
- `retrieved_source_event_ids`
- `context_kind`
- `context_text`
- `is_sufficient`
- `score`
- `label`
- `judge_metadata`
- `evidence_event_ids`
- `evidence_session_ids`

ここでの `is_sufficient` は official retrieval recall ではなく、  
**gold-conditioned retrieval sufficiency** を意味することを schema コメントと metrics provenance で明示します。

### 9.3 `metrics.json`

LongMemEval 用 metrics builder は、LoCoMo と別関数に切り出します。  
必要な headline は次です。

- `overall_answer_accuracy`
- `task_averaged_answer_accuracy`
- `abstention_answer_accuracy`
- `overall_retrieval_sufficiency_accuracy`
- `task_averaged_retrieval_sufficiency_accuracy`

補助値は次です。

- `question_count`
- `non_abstention_question_count`
- `abstention_question_count`
- `average_retrieved_item_count`
- `average_context_token_count`
- `per_type_answer_accuracy`
- `per_type_retrieval_sufficiency_accuracy`

既存の `DatasetMetrics` に不足があれば、LongMemEval 用フィールドを追加します。  
ただし LoCoMo の schema を壊さないよう、optional field と provenance の組で拡張します。

### 9.4 provenance

`metrics.json` の provenance は次に揃えます。

- `protocol = "longmemeval_kioku_v1"`
- `answer_judge_kind = "longmemeval_kioku_answer_llm"`
- `retrieval_judge_kind = "longmemeval_kioku_retrieval_llm"`
- `metric_semantics_version = "longmemeval_kioku_v1"`
- `provisional = false`
- `answer_judge_model`
- `retrieval_judge_model`
- `answer_judge_prompt_id`
- `retrieval_judge_prompt_id`
- `answerer_model`
- `context_tokenizer`

## 10. 実装順

実装順は次を推奨します。

1. LongMemEval 用 prompt 設定と resolved metadata を追加する
2. LongMemEval 用 answer prompt renderer を `longmemeval_kioku_v1` に合わせて実装する
3. LongMemEval 用 answer judge / retrieval judge を実装する
4. `runner/longmemeval_kioku.rs` を追加する
5. CLI の LongMemEval 分岐を新 pipeline に切り替える
6. LongMemEval 用 metrics builder を追加する
7. `answers.jsonl` / `retrieval.jsonl` / `metrics.json` / `run.resolved.json` の出力を調整する
8. `return-all` backend の LongMemEval `prompt_context` を見直す
9. unit test / integration test / regression test を揃える

この順にすると、prompt・judge・runner・metrics の責務境界を崩さずに進められます。

## 11. テスト計画

最低限必要な test は次です。

1. LongMemEval 用 config validation test
2. LongMemEval 用 resolved metadata test
3. `longmemeval.kioku.answer.v1` の prompt rendering test
4. retrieval judge prompt に `question_type` / `question_date` / `is_abstention` / `context_text` が入る test
5. answer judge prompt に `question_type` / `question_date` / `is_abstention` / `generated_answer` が入る test
6. LongMemEval pipeline が `prompt_context` 欠落時に fail-fast する test
7. retrieval judge と answerer が同じ `context_text` を見ている test
8. non-abstention のみが retrieval overall / task-averaged の分母に入る test
9. abstention が answer accuracy では別枠集計される test
10. per-type accuracy の macro average が `task_averaged_*` と一致する test
11. `average_context_token_count` の計算 test
12. LoCoMo pipeline が Phase 5 後も壊れていない回帰 test

LLM 実呼び出しは unit test に入れず、judge runtime は stub / fake transport で検証します。

## 12. リスクと対策

### 12.1 prompt profile 実装との二重管理

Phase 3 の LongMemEval prompt profile 実装と `longmemeval_kioku_v1` の固定 prompt が併存すると、  
LongMemEval の answer path が二重化して混乱しやすいです。

対策:

- LongMemEval CLI path は `longmemeval_kioku_v1` 専用 pipeline に固定する
- 旧 profile 分岐は migration 用に残しても、CLI の標準経路では使わない

### 12.2 metrics schema の過剰一般化

LoCoMo と LongMemEval の metrics は似ていても同一ではありません。  
無理に単一 builder へ統合すると semantics が崩れます。

対策:

- dataset-specific metrics builder を維持する
- 共通 struct には optional field だけを足す
- provenance で meaning を明示する

### 12.3 abstention の扱いミス

LongMemEval では abstention は question type と別軸です。  
ここを per-type 集計へ混ぜると main score の意味が崩れます。

対策:

- metrics 入力段階で `is_abstention` を必ず保持する
- overall / task-averaged / per-type ごとに分母規則を test で固定する

### 12.4 tokenizer 由来の再現性崩れ

`average_context_token_count` は tokenizer 実装が変わると比較できません。

対策:

- Phase 5 では tokenizer を 1 本に固定する
- `run.resolved.json` と `metrics.json` に tokenizer 名を残す

## 13. 完了後の位置づけ

Phase 5 完了後、`crates/evaluate` の benchmark 実装は次の状態になります。

- LoCoMo:
  - `locomo_kioku_v1` で評価可能
- LongMemEval:
  - `longmemeval_kioku_v1` で評価可能
- backend:
  - まだ `return-all` 中心だが、KIOKU backend を差し込むための protocol は固まっている

つまり Phase 6 では、評価仕様の再設計ではなく  
**`KiokuMemoryBackend` をこの protocol に接続すること** が主作業になります。
