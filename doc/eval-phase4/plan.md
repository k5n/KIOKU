# Phase 4 実装計画

## 1. 目的

Phase 4 の目的は、`crates/evaluate` の LoCoMo 実行パスを **KIOKU 用評価仕様 `locomo_kioku_v1`** に合わせて作り替え、暫定の exact match judge ではなく **answer correctness と retrieval sufficiency を同じ question 実行単位で評価できる形** にすることです。

この Phase で先に完成させるのは、次の 4 点です。

1. LoCoMo の評価 semantics を `locomo_kioku_v1` に固定する
2. retrieval judge と answer judge を分離しつつ、同じ `prompt_context.text` を共有する
3. `answers.jsonl` / `retrieval.jsonl` / `metrics.json` / `run.resolved.json` の意味論を `locomo_kioku_v1` に揃える
4. 将来の `KiokuMemoryBackend` がそのまま接続できる I/F とログ schema を先に固める

一方で、この Phase では **KIOKU 本体の backend 実装そのもの** は完了条件に含めません。  
それは全体計画どおり Phase 6 の `KiokuMemoryBackend` で扱います。Phase 4 では、runner / judge / logging / metrics の評価基盤を先に完成させます。

## 2. Phase 4 の完了条件

Phase 4 の完了条件は次です。

1. LoCoMo 実行時の judge semantics が Phase 1 の暫定 exact match ではなく `locomo_kioku_v1` になる
2. LoCoMo の評価対象が category 1-4 のみに固定され、category 5 は集計対象から除外される
3. `AnswerJudge` と `RetrievalJudge`、もしくは同等の 2 系統 judge 抽象が導入されている
4. retrieval judge が `question + gold answers + category + prompt_context.text` を入力に `SUFFICIENT / INSUFFICIENT` を返せる
5. answer judge が `question + gold answers + category + generated answer` を入力に `CORRECT / WRONG` を返せる
6. LoCoMo 実行時は `prompt_context` が必須になり、欠落時は fail-fast で error になる
7. retrieval judge と answerer が同じ `prompt_context.text` を参照する実行順序に整理されている
8. LoCoMo の answer prompt が `locomo.kioku.answer.v1` を使う形で固定される
9. `RetrievalLogRecord` が event-centric schema から memory-centric schema に一般化される
10. `metrics.json` が `overall_answer_accuracy` と `overall_retrieval_sufficiency_accuracy`、および per-category 集計を出せる
11. `run.resolved.json` に answerer 設定とは別に judge 設定と `locomo_kioku_v1` 用 prompt 設定が保存される
12. `PromptContextKind::StructuredFacts` と、それを扱う logging / metrics / tests の型が追加される
13. 実 backend が未完成でも、fixture もしくは mock backend で `locomo_kioku_v1` の end-to-end test が通る
14. LongMemEval の実行パスは既存の暫定 semantics を維持し、Phase 4 の変更で壊れない

## 3. 前提整理

### 3.1 Phase 1-3 まででできていること

Phase 3 までで、共通 runner と TOML 設定基盤、OpenAI 互換 answerer、回答 prompt builder は揃っています。  
現状の `crates/evaluate` には少なくとも次があります。

- `MemoryBackend`
- `PromptBuilder`
- `Answerer`
- `Judge`
- `PromptContext`
- `EvaluatePipeline`

つまり、Phase 4 の主作業は新規 runner の全面書き直しではなく、**judge / output / metrics の意味論変更** です。

### 3.2 現状コードと `locomo_kioku_v1` の主なギャップ

現状コードを基準に見ると、差分は次です。

1. `judge::Judge` は 1 系統しかなく、retrieval judge を持てない
2. `Judgement` は `is_correct` という answer 寄りの命名になっており、retrieval sufficiency を自然に表せない
3. `EvaluatePipeline` は `query -> prompt build -> answer -> single judge` で固定されている
4. `RetrievalLogRecord` は `retrieved_event_ids` 前提で、fact / relation のような memory item を表せない
5. `MetricsReport` は単一 judge 前提の `overall_accuracy` しか持たない
6. `RunConfig` には judge 設定がなく、`run.resolved.json` にも judge provenance を残せない
7. LoCoMo 実行時でも `prompt_context` が optional であり、judge が評価すべき retrieval 対象が曖昧
8. `PromptContextKind` には `StructuredFacts` がない
9. `RetrievedMemory` は `event_id` 中心で、source event と retrieved item 自体を分けられない

### 3.3 Phase 4 で意図的にやらないこと

次は Phase 4 のスコープ外とします。

- LongMemEval の official judge 実装
- LongMemEval の retrieval semantics 実装
- LoCoMo official F1 の再実装
- LoCoMo official retrieval recall の再実装
- judge の複数回実行や多数決
- `KiokuMemoryBackend` の本実装

## 4. 設計原則

### 4.1 Phase 4 は「LoCoMo の protocol 移行」

Phase 4 は汎用 judge 強化ではなく、まず **LoCoMo を `locomo_kioku_v1` に移行する Phase** として扱います。  
そのため、LoCoMo 側は semantics を新仕様へ切り替え、LongMemEval 側は現状維持に留めます。

### 4.2 retrieval judge と answerer は同じ context text を使う

`locomo_kioku_v1` では retrieval の評価対象は retrieved item の配列そのものではなく、**Answerer に実際に渡した `prompt_context.text`** です。  
したがって、runner は次の順序を厳密に守る必要があります。

1. backend から `QueryOutput` を受け取る
2. `prompt_context.text` を固定する
3. retrieval judge にその文字列を渡す
4. 同じ文字列から answer prompt を構築する
5. answerer を実行する
6. answer judge を実行する

### 4.3 LoCoMo 実行時の fallback は認めない

LoCoMo + `locomo_kioku_v1` では、`prompt_context = None` のときに `retrieved` からその場で雑に context を生成して続行する fallback は入れません。  
それを許すと retrieval judge の評価対象が run ごとにぶれます。

この制約は runner だけでなく prompt builder 側でも守る必要があります。  
少なくとも LoCoMo 用 answer prompt 構築では `prompt_context` を必須にし、現在の `retrieved` からの context 合成は廃止します。

### 4.4 schema は KIOKU backend を先回りして固める

Phase 4 の時点で real `KiokuMemoryBackend` がなくても、log と model は KIOKU が返す fact / relation を表現できる形へ先に寄せます。  
これにより Phase 6 で backend を追加するときに output schema を壊さずに済みます。

## 5. 設定と I/F の変更方針

### 5.1 `RunConfig` に judge 設定を追加する

`locomo_kioku_v1` では answerer とは別に judge 用 LLM 設定が必要です。  
そのため、`RunConfig` と TOML schema に `judge` セクションを追加します。

```toml
[judge]
kind = "openai-compatible"

[judge.openai_compatible]
base_url = "http://localhost:11434/v1"
model = "judge-model"
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 512
timeout_secs = 60
max_retries = 3
retry_backoff_ms = 500

[prompt.locomo_kioku]
answer_template_id = "locomo.kioku.answer.v1"
answer_judge_prompt_id = "locomo.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "locomo.kioku.judge.retrieval.v1"
```

v1 では answer judge と retrieval judge の model は同一設定を共有してよいですが、**answerer と judge は別設定** にします。  
これにより judge model の変更が `metrics.json` と `run.resolved.json` に明示的に残ります。

ここでの `judge` 設定は LoCoMo 専用ではなく、`crates/evaluate` の評価基盤全体で利用する共通 judge 設定として導入します。  
将来の LongMemEval でも LLM as a Judge を採る想定のため、config schema と OpenAI 互換 judge runtime は共通化します。

ただし Phase 4 で `locomo_kioku_v1` の dual-judge semantics を適用するのは LoCoMo のみです。  
LongMemEval は judge 設定を受け取れる形までは先に揃えますが、LongMemEval 固有の judge semantics / prompt / metrics を新仕様へ切り替えることは、この Phase の完了条件には含めません。

validation 方針もこの前提に合わせます。

1. LoCoMo + `locomo_kioku_v1` では `[judge]` と `[prompt.locomo_kioku]` を必須にする
2. LongMemEval current path では `[judge]` を config schema 上は許可する
3. ただし LongMemEval では Phase 4 の時点で judge 設定を metrics semantics 変更のトリガにはしない

### 5.2 judge 抽象を 2 系統へ分離する

現在の `Judge` は answer 判定しか想定していないため、Phase 4 では answer と retrieval を分けます。  
内部表現は次のような最小形で十分です。

```rust
pub struct BinaryJudgement {
    pub passed: bool,
    pub score: f32,
    pub label: String,
    pub metadata: serde_json::Value,
}

#[async_trait]
pub trait RetrievalJudge {
    async fn judge_retrieval(
        &self,
        question: &BenchmarkQuestion,
        context: &PromptContext,
    ) -> anyhow::Result<BinaryJudgement>;
}

#[async_trait]
pub trait AnswerJudge {
    async fn judge_answer(
        &self,
        question: &BenchmarkQuestion,
        generated: &GeneratedAnswer,
    ) -> anyhow::Result<BinaryJudgement>;
}
```

ここで重要なのは、内部の汎用構造体は `passed` のような中立名にし、  
`answers.jsonl` では `is_correct`、`retrieval.jsonl` では `is_sufficient` として出し分けることです。

### 5.3 `RetrievedMemory` を一般化する

現状の `RetrievedMemory` は event-centric です。Phase 4 では次のように一般化します。

```rust
pub struct RetrievedMemory {
    pub memory_id: String,
    pub source_event_id: Option<String>,
    pub source_session_id: Option<String>,
    pub score: Option<f32>,
    pub timestamp: Option<String>,
    pub content: String,
    pub metadata: serde_json::Value,
}
```

`ReturnAllMemoryBackend` のような event ベース backend では、`memory_id = event_id` としてマップすれば十分です。  
将来の KIOKU backend では `memory_id` に fact / relation ID を入れ、source trace を `source_event_id` 側へ残します。

Phase 4 の runnable backend として `ReturnAllMemoryBackend` も LoCoMo 用 `prompt_context` を返せるよう更新します。  
LoCoMo 用の暫定 context は retrieved memories の `content` を ingest 順に単純連結した deterministic な文字列で十分です。

### 5.4 `PromptContextKind::StructuredFacts` を追加する

`locomo_kioku_v1` では、KIOKU backend が facts / relations を Answerer に見せる前提を持つため、`PromptContextKind` に `StructuredFacts` を追加します。

ただし Phase 4 の完了条件は、real backend がこの kind を返して本番実行できることではなく、**型・ログ・judge がこの kind を扱えること** です。  
end-to-end 検証は mock backend で十分です。

## 6. runner の変更方針

### 6.1 LoCoMo の question 実行フローを差し替える

LoCoMo 実行時の 1 question の処理を次に固定します。

1. category 1-4 以外なら skip する
2. backend へ `query` を送る
3. `prompt_context` がなければ error にする
4. retrieval judge を実行する
5. `locomo.kioku.answer.v1` で回答 prompt を構築する
6. answerer を実行する
7. answer judge を実行する
8. `retrieval.jsonl` と `answers.jsonl` を同じ question 単位で保存する

この LoCoMo path では `PromptBuilder` も `prompt_context` 必須前提に揃えます。  
`retrieved` だけを見て prompt を組み立てる旧 fallback path は廃止します。

### 6.2 LongMemEval の実行パスは維持する

Phase 4 は LoCoMo 専用の protocol 移行なので、LongMemEval 側まで同じ dual-judge pipeline へ無理に揃えません。  
実装上は次のどちらかに寄せるのが妥当です。

1. `EvaluatePipeline` に protocol 分岐を入れる
2. LoCoMo 向け question evaluator を別コンポーネントへ切り出す

保守性を考えると、**LoCoMo の protocol-specific 実行を `runner/` 配下の別ユニットに切る** 方が後続の LongMemEval Phase と衝突しにくいです。

### 6.3 OpenAI 互換 transport は answerer と共有する

judge も OpenAI 互換 API を使うため、HTTP transport / timeout / retry / JSON parse 周りを answerer 側と重複実装しない方がよいです。  
Phase 4 では、少なくとも次のどちらかを採ります。

1. `answerer/rig_openai.rs` から transport 共通部を抽出する
2. `judge/` 側から再利用できる薄い共通 runtime を追加する

重要なのは、**prompt template は共有しないが transport は共有する** ことです。

## 7. logging と metrics の変更方針

### 7.1 `answers.jsonl`

`answers.jsonl` は answer correctness の結果だけを持ちます。  
Phase 4 では LoCoMo 用 JSON schema を仕様書どおりに固定し、optional な簡略 schema にはしません。  
`answers.jsonl` の 1 line = 1 question とし、少なくとも次を必須で出します。

- `dataset`
- `case_id`
- `question_id`
- `question`
- `generated_answer`
- `gold_answers`
- `label = CORRECT | WRONG`
- `is_correct`
- `score`
- `category`
- `question_type`
- `is_abstention`
- `answer_metadata.template_id = "locomo.kioku.answer.v1"`
- `answer_metadata.answerer_model`
- `judgement_metadata.judge_kind`
- `judgement_metadata.judge_model`
- `judgement_metadata.judge_prompt_id`
- `judgement_metadata.reason`

### 7.2 `retrieval.jsonl`

`retrieval.jsonl` は memory-centric schema に変えます。  
Phase 4 では LoCoMo 用 JSON schema を仕様書どおりに固定し、少なくとも次を必須で持たせます。

- `dataset`
- `case_id`
- `question_id`
- `category`
- `retrieved_count`
- `retrieved_memory_ids`
- `retrieved_source_event_ids`
- `context_kind`
- `context_text`
- `label = SUFFICIENT | INSUFFICIENT`
- `is_sufficient`
- `score`
- `judge_metadata.judge_kind`
- `judge_metadata.judge_model`
- `judge_metadata.judge_prompt_id`
- `judge_metadata.supported_answer`
- `judge_metadata.reason`
- `metadata`

これにより、KIOKU backend が raw event を返さない場合でも retrieval の出力を壊さず保存できます。

### 7.3 `metrics.json`

LoCoMo では `MetricsReport` を単一 accuracy 前提の形から拡張し、次を出します。

- `protocol = "locomo_kioku_v1"`
- `provenance.answer_judge_kind`
- `provenance.retrieval_judge_kind`
- `provenance.metric_semantics_version = "locomo_kioku_v1"`
- `provenance.provisional = false`
- `provenance.locomo_overall_scope = "category_1_4"`
- `provenance.answer_judge_model`
- `provenance.retrieval_judge_model`
- `provenance.answer_judge_prompt_id`
- `provenance.retrieval_judge_prompt_id`
- `provenance.answerer_model`
- `metrics.question_count`
- `metrics.overall_answer_accuracy`
- `metrics.overall_retrieval_sufficiency_accuracy`
- `metrics.average_retrieved_item_count`
- `metrics.per_category_answer_accuracy`
- `metrics.per_category_retrieval_sufficiency_accuracy`

LongMemEval の現行 metrics と完全統合しようとすると型が崩れやすいため、Phase 4 では **LoCoMo 用 metrics の意味論を優先し、必要なら dataset ごとに内部 builder を分ける** 方針を採ります。

また LoCoMo 用 `metrics.json` は current の `overall_accuracy` / `adversarial_accuracy` / `per_type_accuracy` を引きずらず、`locomo_kioku_v1` の exact schema に合わせます。

### 7.4 `run.resolved.json`

`run.resolved.json` についても LoCoMo 用 provenance を exact に残します。  
少なくとも次を保存します。

- `dataset`
- `backend`
- `answerer`
- `judge`
- `retrieval`
- `prompt.locomo_kioku.answer_template_id`
- `prompt.locomo_kioku.answer_judge_prompt_id`
- `prompt.locomo_kioku.retrieval_judge_prompt_id`

これにより、answerer 設定と judge 設定、prompt version 群を分離して後から run provenance を比較できます。

## 8. 想定ファイル構成

Phase 4 で主に追加・更新する対象は次です。

```text
crates/evaluate/src/
├── config/
│   ├── metadata.rs
│   ├── resolve.rs
│   ├── toml.rs
│   ├── types.rs
│   └── validate.rs
├── judge/
│   ├── mod.rs
│   ├── traits.rs
│   ├── locomo_kioku.rs
│   └── openai_compatible.rs
├── model/
│   ├── metrics.rs
│   └── retrieval.rs
├── prompt/
│   ├── answer.rs
│   └── context.rs
└── runner/
    ├── metrics.rs
    ├── output.rs
    └── pipeline.rs
```

必要に応じて `runner/locomo_kioku.rs` のような protocol 専用ユニットへ分けても構いません。  
この Phase ではモジュールの分離よりも、**LoCoMo と LongMemEval の semantics を混ぜないこと** を優先します。

## 9. 実装順序

1. `RetrievedMemory`、`RetrievalLogRecord`、`MetricsReport` の型を `locomo_kioku_v1` に合わせて一般化する
2. `PromptContextKind::StructuredFacts` を追加する
3. `ReturnAllMemoryBackend` が LoCoMo でも deterministic な `prompt_context` を返せるようにする
4. TOML / `RunConfig` / `run.resolved.json` に共通 judge 設定と LoCoMo prompt 設定を追加する
5. LoCoMo 用 prompt builder から `retrieved` ベースの context 合成 fallback を削除する
6. answer judge / retrieval judge の抽象を導入する
7. OpenAI 互換 judge runtime を実装し、JSON-only prompt の parse を含めて fail-fast にする
8. LoCoMo の runner 実行順序を `query -> retrieval judge -> answer -> answer judge` に組み替える
9. `answers.jsonl` / `retrieval.jsonl` / `metrics.json` / `run.resolved.json` の exact schema writer を更新する
10. category 1-4 のみを対象とする集計ロジックを実装する
11. mock backend で `StructuredFacts` context を返す end-to-end test を追加する
12. LongMemEval の既存 path が壊れていないことを回帰 test で確認する

## 10. テスト計画

Phase 4 で最低限入れるテストは次です。

1. LoCoMo 実行時に category 5 が集計から除外される test
2. LoCoMo 実行時に `prompt_context = None` なら fail-fast する test
3. retrieval judge が `SUFFICIENT / INSUFFICIENT` JSON を parse できる test
4. answer judge が `CORRECT / WRONG` JSON を parse できる test
5. retrieval judge と answerer が同じ `context_text` を参照する test
6. `locomo.kioku.answer.v1` が LoCoMo answer prompt に選ばれる test
7. LoCoMo 用 prompt builder が `prompt_context = None` を reject し、`retrieved` から context を合成しない test
8. `ReturnAllMemoryBackend` が LoCoMo でも deterministic な `prompt_context.text` を返す test
9. `RetrievalLogRecord` が exact schema どおり `retrieved_memory_ids` と `retrieved_source_event_ids`、`context_text`、judge provenance を出す test
10. LoCoMo 用 `MetricsReport` が exact schema どおり answer / retrieval の 2 系統 accuracy と provenance を出す test
11. `run.resolved.json` に judge 設定と LoCoMo prompt ID 群が保存される test
12. mock `StructuredFacts` backend で `locomo_kioku_v1` の end-to-end が通る test
13. LongMemEval の既存 runner path が Phase 4 後も通る test
14. judge API failure や JSON parse failure を不正解ではなく run failure にする test

## 11. リスクと対策

### 11.1 real `KiokuMemoryBackend` がまだないリスク

対策:

- Phase 4 の完了条件から backend 本実装を外す
- `StructuredFacts` を返す mock backend で protocol を先に固定する
- backend に要求する `prompt_context` 契約を文書と型に落とす

### 11.2 judge を 2 系統にしたことで runner が複雑化するリスク

対策:

- LoCoMo 専用の question evaluator を切り出し、LongMemEval と混ぜない
- answer log と retrieval log を同じ question 実行単位で生成する

### 11.3 judge 設定を共通化したことで LongMemEval まで巻き込んで壊すリスク

対策:

- `judge` config は共通導入するが、Phase 4 で新 semantics を適用するのは LoCoMo のみに限定する
- validation を dataset / protocol 単位で分け、LongMemEval current path の互換性を維持する
- LoCoMo と LongMemEval の runner / metrics / prompt test を分離する

### 11.4 output schema 変更で既存テストが広く壊れるリスク

対策:

- model 型から先に変更し、writer と metrics を後から追従させる
- LongMemEval は現行 semantics のまま維持し、dataset ごとの test を分ける
- LoCoMo 用 JSON schema は exact に固定し、曖昧な optional field を増やさない

### 11.5 LLM judge の JSON parse が不安定なリスク

対策:

- judge prompt を JSON-only へ固定する
- parse failure は wrong 扱いにせず run failure とする
- `judge_prompt_id` と `judge_model` を provenance に必ず残す

## 12. Phase 4 完了後に着手するもの

Phase 4 の次に進める対象は次です。

1. LongMemEval の KIOKU 用評価仕様実装
2. `KiokuMemoryBackend` の実装と `StructuredFacts` の本番接続
3. baseline backend の追加
4. checkpoint / resume、並列実行、cost 計測などの実験基盤強化

Phase 4 で最も重要なのは、LoCoMo の semantics を暫定値から切り離し、  
**KIOKU の retrieval と answer を別々に比較できる protocol を runner に固定すること** です。
