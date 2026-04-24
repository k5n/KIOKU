# KIOKU の LoCoMo 評価仕様 (`locomo_kioku_v1`)

## 1. 目的

この文書は、KIOKU の LoCoMo 評価プロトコルとして採用する **`locomo_kioku_v1`** の仕様を定義するものです。  
目的は、LoCoMo の公式実装や既存論文との数値比較ではなく、**KIOKU 自身の改善効果を安定して測定すること**です。

そのため、この仕様では次を優先します。

- 実装の単純さ
- 評価結果の解釈のしやすさ
- KIOKU のような「Atomic Facts + 関係リンク」を返す記憶層への適合
- answer と retrieval の失敗を切り分けられること

この仕様では、LoCoMo 公式の deterministic F1 には合わせません。  
また、MAGMA のような best-of-N や gold-aware reranking も採用しません。

## 2. この仕様で採る立場

`locomo_kioku_v1` は、LoCoMo を **「長期会話記憶ベンチマーク」** として利用しますが、採点方法は KIOKU 用に再定義します。

基本方針は次です。

1. answer correctness は LLM judge で判定する
2. retrieval quality も LLM judge で判定する
3. retrieval の評価対象は「Answerer が実際に見た最終コンテキスト文字列」とする
4. KIOKU backend 間の比較では、backend 以外の条件を固定する

つまり、この仕様における retrieval の評価は、「evidence turn を何件拾えたか」ではなく、  
**KIOKU が返した facts / relations のコンテキストだけで、その質問に答えられる状態か** を見るものとします。

## 3. v1 のスコープ

`locomo_kioku_v1` で **まず実装するもの** は次だけです。

- LoCoMo の `category 1-4` を対象にする
- answer correctness を LLM judge で採点する
- retrieval sufficiency を LLM judge で採点する
- overall と per-category の集計を出す
- `answers.jsonl`, `retrieval.jsonl`, `metrics.json`, `run.resolved.json` を出力する

逆に、次は **v1 のスコープ外** とします。

- `category 5` の adversarial evaluation
- LoCoMo 公式 F1 の再実装
- LoCoMo 公式 retrieval recall の再実装
- judge の 3 回実行や多数決
- human annotation による judge 校正
- retrieved facts が元会話に忠実かどうかの faithfulness judge
- `locomo_official_v1` の同時実装

## 4. category の扱い

`locomo_kioku_v1` では、LoCoMo のうち **category 1-4 のみ**を評価対象とします。

- `1`: Multi-hop
- `2`: Temporal
- `3`: Open-domain
- `4`: Single-hop

`category 5` は v1 では除外します。

理由は次です。

- task の性質が他カテゴリと異なる
- `answer` と `adversarial_answer` の不整合がある
- abstention の judge は別仕様として切り出した方が単純

したがって、この仕様における LoCoMo の overall は、常に **category 1-4 のみ**を対象にします。

## 5. 評価単位

LoCoMo の評価単位は従来どおりです。

- 1 sample = 1 conversation
- 1 sample 内に複数の QA がある
- 1 sample の全会話を ingest した後に、各 question を query する

したがって runner の基本単位は次です。

1. case を 1 つ初期化する
2. case 内の全 event を時系列順に ingest する
3. case 内の各 question について query する
4. retrieval judge と answer judge を行う
5. 全 question を集計する

## 6. KIOKU における retrieval の評価対象

KIOKU は raw message をそのまま返すとは限りません。  
返すのは、主に次です。

- Atomic Facts
- fact 間の関係リンク
- fact や relation を説明する補助テキスト

このため `locomo_kioku_v1` では、retrieval の評価対象を **retrieved items の集合そのもの** ではなく、
**Answerer に実際に渡した最終コンテキスト文字列** に固定します。

つまり、この仕様で judge が見る retrieval 入力は、backend が返した
**`QueryOutput.prompt_context`** です。

retrieval judge が使うのは **`prompt_context.text`** です。

### 6.1 なぜ `prompt_context.text` を使うのか

理由は単純です。

- backend が facts と links をどう並べるかで answerability が変わる
- query の生結果より、Answerer が実際に見た文脈を評価した方が解釈しやすい
- 「検索は良いが prompt 化で壊れた」も retrieval 側の失敗として拾える

したがって、`locomo_kioku_v1` における retrieval は、
**検索アルゴリズムそのもの** だけでなく、
**検索結果を Answerer 用コンテキストへ整形する段階まで含めた retrieval stage** とみなします。

### 6.2 KIOKU backend への要求

KIOKU backend は LoCoMo 実行時に evaluation-ready な `QueryOutput.prompt_context` を
**必ず**返すものとします。

v1 の仕様では `prompt_context` は optional ではありません。  
fallback で raw retrieval item を render する official 互換動作も使いません。

また、`prompt_context.text` は次の性質を満たす必要があります。

- facts と relations の区別が読める
- item の並び順が決定的である
- 同じ backend と同じ入力なら同じ文字列が生成される
- Answerer と retrieval judge が同じ文字列を参照する

### 6.3 `PromptContextKind`

LoCoMo + KIOKU 用には、`PromptContextKind` に新たに `StructuredFacts` を追加するのが望ましいです。

```rust
pub enum PromptContextKind {
    RetrievedMemories,
    StructuredFacts,
    NoRetrieval,
    HistoryChats,
    HistoryChatsWithFacts,
    FactsOnly,
}
```

v1 実装では、少なくとも metadata で「これは structured facts 用 context である」と識別できれば動作します。  
ただし型で区別できる方が安全です。

## 7. 評価フロー

`locomo_kioku_v1` の 1 question あたりの評価フローは次です。

1. backend が `QueryOutput` を返す
2. runner が `prompt_context.text` を取得する
3. retrieval judge が `question + gold answer + prompt_context.text` を採点する
4. answerer が同じ `prompt_context.text` から最終 answer を生成する
5. answer judge が `question + gold answer + generated answer` を採点する
6. retrieval log と answer log を保存する

重要なのは、retrieval judge と answerer が **同じ context text** を見ることです。

擬似コードで書くと次です。

```rust
for case in locomo_cases {
    backend.reset(case.scope()).await?;

    for event in case.events {
        backend.ingest(event).await?;
    }

    for question in case.questions.filter(category in 1..=4) {
        let query_output = backend.query(question.to_query_input()).await?;
        let context = query_output.prompt_context;

        let retrieval_judgement = retrieval_judge.judge(
            question,
            &context.text,
        ).await?;

        let prompt = locomo_kioku_prompt_builder.build(question, &context.text)?;
        let generated_answer = answerer.answer(prompt).await?;

        let answer_judgement = answer_judge.judge(
            question,
            &generated_answer,
        ).await?;

        save_logs(...);
    }
}
```

## 8. Answerer の仕様

`locomo_kioku_v1` では、Answerer は 1 回だけ実行します。

- best-of-N はしない
- self-consistency はしない
- reranking はしない
- gold answer を answer selection に使わない

Answerer の温度は `0` を原則とします。

### 8.1 Answerer prompt の要求

LoCoMo + KIOKU 用の Answerer prompt は、最低限次を満たす必要があります。

- 与えられた memory context だけに基づいて答える
- 外部知識で補わない
- 答えは短く返す
- 説明や chain-of-thought は出さない
- context が不足していると判断したときは、決め打ちの sentinel を返す

v1 では、insufficient context の sentinel は次で固定します。

- `NOT_ENOUGH_MEMORY`

### 8.2 推奨テンプレート

- template id: `locomo.kioku.answer.v1`

system prompt:

```text
You answer questions using only the provided memory context.
Do not use external knowledge.
If the memory context is insufficient, answer exactly: NOT_ENOUGH_MEMORY
Return only the final answer as a short phrase.
```

user prompt:

```text
Memory context:
{context_text}

Question:
{question}
```

## 9. Judge の仕様

`locomo_kioku_v1` では judge はすべて LLM judge に寄せます。  
ただし v1 では **2 種類の binary judge** だけを実装します。

1. retrieval sufficiency judge
2. answer correctness judge

両者は同じ judge runtime を共有して構いません。  
違うのは prompt template だけです。

### 9.1 共通ルール

judge の共通ルールは次です。

- model は config で指定する
- temperature は `0`
- 1 question あたり 1 回だけ判定する
- transport retry は許可する
- parse 不能や API failure は run failure とする
- judge の結果を wrong / insufficient に丸めて握りつぶさない

この仕様では、judge の品質は **使用モデル + prompt version** に依存します。  
したがって、比較可能なのは同じ judge 条件で取った run 同士だけです。

## 10. Retrieval Sufficiency Judge

### 10.1 目的

retrieval sufficiency judge は、  
**「この retrieval context だけで、gold と同等の答えに到達できるか」**  
を判定します。

ここで見るのは answer の出来ではなく、retrieval context の十分性です。

### 10.2 入力

入力は次です。

- question
- gold answers
- category
- retrieved context text

judge は generated answer を見ません。

### 10.3 ラベル

ラベルは 2 値です。

- `SUFFICIENT`
- `INSUFFICIENT`

`SUFFICIENT` の定義は次です。

- provided context だけで、gold answer と意味的に同等な答えを導ける
- 表現の言い換えは許す
- 日付の表現揺れは許す
- ただし、必要な entity / relation / time anchor が欠けているなら `INSUFFICIENT`

特に次のケースは `INSUFFICIENT` とします。

- 必要な fact が抜けている
- 必要な relation が抜けている
- temporal question で基準日やイベント時刻が足りない
- context に近い話題はあるが、gold answer に到達できる根拠が不足している

### 10.4 出力 JSON

template id: `locomo.kioku.judge.retrieval.v1`

出力は次の JSON に固定します。

```json
{
  "label": "SUFFICIENT",
  "supported_answer": "May 2019",
  "reason": "The context contains the event and enough temporal information to derive the answer."
}
```

各フィールドの意味は次です。

- `label`
  - `SUFFICIENT` or `INSUFFICIENT`
- `supported_answer`
  - context だけから導けると judge が考えた短い答え
  - `INSUFFICIENT` の場合は `null` でよい
- `reason`
  - 1 文の短い理由

### 10.5 推奨 prompt

system prompt:

```text
You are an evaluator of retrieval quality for a conversational memory benchmark.
Judge whether the provided memory context alone is sufficient to answer the question with a gold-equivalent answer.
Do not judge writing quality. Do not use external knowledge beyond basic language understanding.
Return JSON only.
```

user prompt:

```text
Question:
{question}

Gold answers:
{gold_answers_json}

Question category:
{category}

Retrieved memory context:
{context_text}

Label the context as SUFFICIENT if the context alone contains enough information to derive a correct answer equivalent to one of the gold answers.
Otherwise label it INSUFFICIENT.

Return JSON with:
- label: SUFFICIENT or INSUFFICIENT
- supported_answer: short answer or null
- reason: one short sentence
```

## 11. Answer Correctness Judge

### 11.1 目的

answer correctness judge は、  
**generated answer が gold answer と意味的に一致しているか**  
を判定します。

### 11.2 入力

入力は次です。

- question
- gold answers
- category
- generated answer

judge は retrieval context を見ません。

### 11.3 ラベル

ラベルは 2 値です。

- `CORRECT`
- `WRONG`

判定方針は次です。

- 言い換えは許す
- 日付や期間の表現差は許す
- 部分的に触れているだけでは不正解
- entity mismatch は不正解
- temporal question で時期がずれていれば不正解
- `NOT_ENOUGH_MEMORY` は通常不正解

### 11.4 出力 JSON

template id: `locomo.kioku.judge.answer.v1`

```json
{
  "label": "CORRECT",
  "reason": "The generated answer matches the gold answer semantically."
}
```

### 11.5 推奨 prompt

system prompt:

```text
You are an evaluator of answer correctness for a conversational memory benchmark.
Judge whether the generated answer is semantically equivalent to any gold answer.
Be tolerant to wording differences, but strict about wrong entities, wrong dates, and incomplete answers.
Return JSON only.
```

user prompt:

```text
Question:
{question}

Gold answers:
{gold_answers_json}

Question category:
{category}

Generated answer:
{generated_answer}

Return JSON with:
- label: CORRECT or WRONG
- reason: one short sentence
```

## 12. v1 の headline metrics

`locomo_kioku_v1` の headline metrics は次の 2 つだけです。

1. `overall_answer_accuracy`
2. `overall_retrieval_sufficiency_accuracy`

これだけで、まずは次を判断できます。

- KIOKU の retrieval が十分だったか
- retrieval が十分でも最終 answer で落ちていないか

### 12.1 overall の定義

overall は **category 1-4 の pooled micro average** とします。

つまり次です。

- `overall_answer_accuracy`
  - `CORRECT` と判定された question 数 / 全 evaluated question 数
- `overall_retrieval_sufficiency_accuracy`
  - `SUFFICIENT` と判定された question 数 / 全 evaluated question 数

### 12.2 per-category

headline は overall ですが、レポートには次も出します。

- category 1 の answer accuracy
- category 2 の answer accuracy
- category 3 の answer accuracy
- category 4 の answer accuracy
- category 1 の retrieval sufficiency accuracy
- category 2 の retrieval sufficiency accuracy
- category 3 の retrieval sufficiency accuracy
- category 4 の retrieval sufficiency accuracy

### 12.3 補助カウント

v1 では次の補助値を出します。

- `question_count`

context size の比較は raw retrieval 件数ではなく、必要に応じて `prompt_context.text`
由来の token 指標で扱います。

## 13. v1 で出力するファイル

`locomo_kioku_v1` では、少なくとも次を出力します。

- `answers.jsonl`
- `retrieval.jsonl`
- `metrics.json`
- `run.resolved.json`

## 14. `answers.jsonl` の仕様

1 line = 1 question です。

最低限次を持ちます。

```json
{
  "dataset": "locomo",
  "case_id": "locomo:sample:0",
  "question_id": "locomo:sample:0:q:12",
  "question": "When did ...?",
  "generated_answer": "May 2019",
  "gold_answers": ["May 2019"],
  "is_correct": true,
  "score": 1.0,
  "label": "CORRECT",
  "category": 2,
  "question_type": null,
  "is_abstention": false,
  "answer_metadata": {
    "template_id": "locomo.kioku.answer.v1",
    "answerer_model": "..."
  },
  "judgement_metadata": {
    "judge_kind": "locomo_kioku_answer_llm",
    "judge_model": "...",
    "judge_prompt_id": "locomo.kioku.judge.answer.v1",
    "reason": "The generated answer matches the gold answer semantically."
  }
}
```

## 15. `retrieval.jsonl` の仕様

1 line = 1 question です。

KIOKU backend では raw event ではなく fact / relation を返しうるため、retrieval log は current Phase 1 の event-centric schema より少し一般化する必要があります。

最低限次を持ちます。

```json
{
  "dataset": "locomo",
  "case_id": "locomo:sample:0",
  "question_id": "locomo:sample:0:q:12",
  "category": 2,
  "context_kind": "structured-facts",
  "context_text": "1. [fact] ...",
  "is_sufficient": true,
  "score": 1.0,
  "label": "SUFFICIENT",
  "judge_metadata": {
    "judge_kind": "locomo_kioku_retrieval_llm",
    "judge_model": "...",
    "judge_prompt_id": "locomo.kioku.judge.retrieval.v1",
    "supported_answer": "May 2019",
    "reason": "The context contains the event and enough temporal information to derive the answer."
  },
  "metadata": {
    "backend": "kioku"
  }
}
```

### 15.1 current schema との差分

current `RetrievalLogRecord` は raw retrieval item を前提にしていました。  
Phase 5.6 後の v1 ではそれをやめ、次を正規記録します。

- `context_kind`
- `context_text`
- retrieval judge の結果
- backend-specific metadata

## 16. `metrics.json` の仕様

`metrics.json` は protocol 固有の provenance を明示する必要があります。

最低限次を持ちます。

```json
{
  "dataset": "locomo",
  "protocol": "locomo_kioku_v1",
  "answer_judge_kind": "locomo_kioku_answer_llm",
  "retrieval_judge_kind": "locomo_kioku_retrieval_llm",
  "metric_semantics_version": "locomo_kioku_v1",
  "provisional": false,
  "locomo_overall_scope": "category_1_4",
  "answer_judge_model": "...",
  "retrieval_judge_model": "...",
  "answer_judge_prompt_id": "locomo.kioku.judge.answer.v1",
  "retrieval_judge_prompt_id": "locomo.kioku.judge.retrieval.v1",
  "answerer_model": "...",
  "metrics": {
    "question_count": 1540,
    "overall_answer_accuracy": 0.74,
    "overall_retrieval_sufficiency_accuracy": 0.81,
    "per_category_answer_accuracy": {
      "1": { "correct": 201, "total": 282, "accuracy": 0.7128 },
      "2": { "correct": 250, "total": 321, "accuracy": 0.7788 },
      "3": { "correct": 63, "total": 96, "accuracy": 0.6562 },
      "4": { "correct": 625, "total": 841, "accuracy": 0.7431 }
    },
    "per_category_retrieval_sufficiency_accuracy": {
      "1": { "correct": 221, "total": 282, "accuracy": 0.7837 },
      "2": { "correct": 272, "total": 321, "accuracy": 0.8474 },
      "3": { "correct": 70, "total": 96, "accuracy": 0.7291 },
      "4": { "correct": 685, "total": 841, "accuracy": 0.8145 }
    }
  }
}
```

### 16.1 provenance の扱い

judge model や prompt を変えると数値の意味も変わります。  
したがって provenance には少なくとも次を必ず残します。  
実装上はこれらの field は `metrics.json` top-level に flatten され、`provenance` object にはネストされません。

- protocol 名
- answer judge model
- retrieval judge model
- answer judge prompt id
- retrieval judge prompt id
- answerer model
- overall scope

## 17. config の最小追加仕様

current `crates/evaluate` は answerer config はありますが judge config がありません。  
`locomo_kioku_v1` では judge 用 config を追加します。

v1 では judge model は answer judge / retrieval judge で共通とします。

```toml
[run]
input = "..."
output_dir = "..."

[backend]
kind = "kioku"

[answerer]
kind = "openai-compatible"

[answerer.openai-compatible]
base_url = "..."
model = "..."
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 128
timeout_secs = 60
max_retries = 3
retry_backoff_ms = 500

[judge]
kind = "openai-compatible"

[judge.openai-compatible]
base_url = "..."
model = "..."
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 512
timeout_secs = 60
max_retries = 3
retry_backoff_ms = 500

[retrieval]
max_items = 12

[benchmark.locomo]
answer_template_id = "locomo.kioku.answer.v1"
answer_judge_prompt_id = "locomo.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "locomo.kioku.judge.retrieval.v1"
```

## 18. current codebase に対して必要な変更

`locomo_kioku_v1` を実装するには、少なくとも次の変更が必要です。

### 18.1 Judge を 2 系統にする

current pipeline は `Judge` を 1 回だけ呼びます。  
v1 では次の 2 つが必要です。

- `RetrievalJudge`
- `AnswerJudge`

たとえば次のように分けるのが自然です。

```rust
#[async_trait]
pub trait RetrievalJudge {
    async fn judge_retrieval(
        &self,
        question: &BenchmarkQuestion,
        context: &PromptContext,
    ) -> anyhow::Result<Judgement>;
}

#[async_trait]
pub trait AnswerJudge {
    async fn judge_answer(
        &self,
        question: &BenchmarkQuestion,
        generated: &GeneratedAnswer,
    ) -> anyhow::Result<Judgement>;
}
```

### 18.2 pipeline を拡張する

current `EvaluatePipeline` は answer judgement しか持ちません。  
v1 では retrieval judgement も保存する必要があります。

### 18.3 LoCoMo 実行時に `prompt_context` を必須化する

KIOKU backend で optional な `prompt_context` を許すと、retrieval judge が評価すべき
対象が曖昧になります。  
そのため LoCoMo + KIOKU 実行では `prompt_context` を型として必須にします。

### 18.4 retrieval log schema を `prompt_context` 中心へ整理する

raw retrieval item を共通 schema に残すと backend 契約が重くなります。  
そのため Phase 5.6 以降の retrieval log は、`prompt_context` と judge 結果を中心に
記録します。

### 18.5 metrics schema を拡張する

current `MetricsReport` は 1 種類の judge しか想定していません。  
v1 では answer と retrieval の 2 系統を分けて持つ必要があります。

## 19. v1 で採らないもの

意図的に採らないものを再掲します。

- `category 5`
- official F1
- official retrieval recall
- judge 3 runs
- best-of-N
- human eval
- grounding / faithfulness judge

これらを一度に入れると、KIOKU の最初の end-to-end 評価仕様としては複雑になり過ぎます。  
`locomo_kioku_v1` は、まず **「KIOKU の retrieval context は答えるのに十分か」「最終 answer は正しいか」** の 2 点だけを安定して測ることを目的にします。

## 20. まとめ

`locomo_kioku_v1` の本質は次の 3 点です。

1. LoCoMo は category 1-4 のみを使う
2. retrieval も answer も LLM judge で binary に採点する
3. retrieval の評価対象は、KIOKU が Answerer に見せた最終 context text そのものにする

この仕様なら、KIOKU のような構造化記憶に対しても、
raw message retrieval に引きずられずに評価できます。  
また、実装も「1 answerer + 2 binary judges + 2 つの headline metrics」に絞れるため、最初の仕様として十分に単純です。
