# KIOKU の LongMemEval 評価仕様 (`longmemeval_kioku_v1`)

## 1. 目的

この文書は、KIOKU の LongMemEval 評価プロトコルとして採用する **`longmemeval_kioku_v1`** の仕様を定義するものです。  
目的は LongMemEval 公式 leaderboard の忠実再現ではなく、**KIOKU 自身の改善効果を安定して測定すること**です。

そのため、この仕様では次を優先します。

- 実装の単純さ
- backend 間比較の再現性
- KIOKU のような structured memory backend への適合
- answer と retrieval の失敗の切り分け
- LongMemEval の task semantics を壊さないこと

この仕様では、official retrieval recall / nDCG を headline metric にはしません。  
また、EverMemOS のような 3 judge average や MAGMA のような best-of-N も採用しません。

## 2. この仕様で採る立場

`longmemeval_kioku_v1` は、LongMemEval を **「長期会話記憶ベンチマーク」** として利用しますが、採点方法と reporting は KIOKU 用に再定義します。

基本方針は次です。

1. answer correctness は LLM judge で判定する
2. retrieval quality も LLM judge で判定する
3. retrieval の評価対象は「Answerer が実際に見た最終コンテキスト文字列」とする
4. LongMemEval 公式の question-type semantics は judge rubric に残す
5. KIOKU backend 間の比較では backend 以外の条件を固定する

つまり、この仕様における retrieval の評価は、「annotated evidence session / turn を top-k にどれだけ拾えたか」ではなく、  
**KIOKU が返した context だけで、その question type の rubric に従って答えられる状態か** を見るものとします。

## 3. v1 のスコープ

`longmemeval_kioku_v1` で **まず実装するもの** は次だけです。

- LongMemEval `S-cleaned` を main dataset として評価する
- answer correctness を LLM judge で採点する
- gold-conditioned retrieval sufficiency を LLM judge で採点する
- LongMemEval official の question-type semantics を judge rubric に反映する
- overall / task-averaged / per-type / abstention の集計を出す
- `answers.jsonl`, `retrieval.jsonl`, `metrics.json`, `run.resolved.json` を出力する

逆に、次は **v1 のスコープ外** とします。

- LongMemEval official retrieval recall / nDCG の headline 化
- 公式 leaderboard 数値との厳密比較
- judge の 3 回実行や多数決
- human annotation による judge 校正
- retrieved facts の faithfulness judge
- `longmemeval_official_v1` の同時実装
- `M-cleaned` を main score として扱うこと

## 4. データセットの扱い

`longmemeval_kioku_v1` の main dataset は **`longmemeval_s_cleaned.json`** とします。

- main score: `longmemeval_s_cleaned.json`
- optional stress test: `longmemeval_m_cleaned.json`
- optional sanity check: `longmemeval_oracle.json`

理由は次です。

- `S-cleaned` が現在の公式 cleaned release の基本条件である
- `M-cleaned` は stress test として有用だが、v1 の main comparison には重い
- `oracle` は retrieval の上限確認には使えるが、KIOKU backend 比較の main score には向かない

したがって、この仕様における **LongMemEval overall は常に `S-cleaned` を指す** ものとします。  
`M-cleaned` や `oracle` を実行する場合は、別 run として並べて表示します。

## 5. 評価単位

LongMemEval の評価単位は official と同じです。

- 1 entry = 1 question
- 1 entry の中に、その question に対応する `haystack_sessions` が埋め込まれている
- entry ごとに独立した memory state を再構築する

したがって runner の基本単位は次です。

1. case を 1 つ初期化する
2. case 内の全 event を時系列順に ingest する
3. case の 1 question に対して query する
4. retrieval judge と answer judge を行う
5. 全 question を集計する

LoCoMo と違って、**会話 1 本に複数 question をぶら下げる構造ではない** ことに注意が必要です。

## 6. ingest と時刻の扱い

LongMemEval では各 entry に次が入っています。

- `haystack_session_ids`
- `haystack_dates`
- `haystack_sessions`
- `question_date`

したがって ingest は次の順で行います。

1. `haystack_dates` の昇順で session を処理する
2. 同値なら `haystack_session_ids` で決定的に並べる
3. 各 session 内では turn 配列順に処理する
4. turn timestamp は session timestamp から疑似生成する
5. 全 session を入れ終わった後に `query` を呼ぶ

`question_date` は検索 cutoff ではなく、**質問がどの時点の世界状態を問うているか** を表す参照時刻です。  
したがって `query input`、`answer prompt`、judge rubric のすべてで参照できるようにします。

## 7. question type と abstention

LongMemEval 公式の question type は次です。

- `single-session-user`
- `single-session-assistant`
- `single-session-preference`
- `temporal-reasoning`
- `knowledge-update`
- `multi-session`

加えて、**`question_id` が `_abs` で終わるものは abstention question** です。  
これは `question_type` とは別軸です。

`longmemeval_kioku_v1` では、この official の task semantics は維持します。  
ただし scoring と logging は KIOKU 用 protocol に寄せます。

重要なのは、abstention は **通常 QA と別の失敗モード** を持つことです。

- 通常 question
  - 必要な記憶を取得できれば正答に到達できるかを測る
- abstention question
  - 記憶が不足しているときに、推測や hallucination をせず回答を差し控えられるかを測る

したがって v1 では、abstention は記録・報告はするものの、**main backend score を構成する通常 QA 指標とは分離して扱う**ものとします。

## 8. KIOKU における retrieval の評価対象

KIOKU は raw chat history をそのまま返すとは限りません。  
返すのは、主に次です。

- Atomic Facts
- fact 間の関係リンク
- facts / relations を説明する補助テキスト
- 必要に応じて history snippets を含む複合 context

このため `longmemeval_kioku_v1` では、retrieval の評価対象を **retrieved item の集合そのもの** ではなく、  
**Answerer に実際に渡した最終コンテキスト文字列** に固定します。

つまり、この仕様で judge が見る retrieval 入力は、backend が返した
**`QueryOutput.prompt_context`** です。

retrieval judge が使うのは **`prompt_context.text`** です。

### 8.1 なぜ `prompt_context.text` を使うのか

理由は次です。

- KIOKU backend は official の session retrieval とは違う粒度で記憶を返しうる
- facts / links の並べ方しだいで answerability が変わる
- retrieval 自体が良くても prompt 化で壊れる場合がある
- annotated evidence の recall だけでは structured memory の usefulness を過小評価しやすい

したがって、この仕様における retrieval は、  
**検索アルゴリズムそのもの** だけでなく、  
**検索結果を Answerer 用コンテキストへ整形する段階まで含めた retrieval stage** とみなします。

### 8.2 KIOKU backend への要求

KIOKU backend は LongMemEval 実行時に evaluation-ready な `QueryOutput.prompt_context` を
**必ず**返すものとします。

v1 の仕様では `prompt_context` は optional ではありません。  
fallback で raw retrieval item を render する provisional 動作も使いません。

また、`prompt_context.text` は次の性質を満たす必要があります。

- item の並び順が決定的である
- 同じ backend と同じ入力なら同じ文字列が生成される
- question_date と矛盾しない time reference を持てる
- Answerer と retrieval judge が同じ文字列を参照する

## 9. 評価フロー

`longmemeval_kioku_v1` の 1 question あたりの評価フローは次です。

1. backend が `QueryOutput` を返す
2. runner が `prompt_context.text` を取得する
3. retrieval judge が `question + gold answer + question_type + question_date + prompt_context.text` を採点する
4. answerer が同じ `prompt_context.text` から最終 answer を生成する
5. answer judge が `question + gold answer + question_type + question_date + generated answer` を採点する
6. retrieval log と answer log を保存する

重要なのは、retrieval judge と answerer が **同じ context text** を見ることです。

擬似コードで書くと次です。

```rust
for case in longmemeval_cases {
    backend.reset(case.scope()).await?;

    for event in case.events {
        backend.ingest(event).await?;
    }

    let question = case.only_question();
    let query_output = backend.query(question.to_query_input()).await?;
    let context = query_output.prompt_context;

    let retrieval_judgement = retrieval_judge.judge_retrieval(
        question,
        &context,
    ).await?;

    let prompt = longmemeval_kioku_prompt_builder.build(question, &context.text)?;
    let generated_answer = answerer.answer(prompt).await?;

    let answer_judgement = answer_judge.judge_answer(
        question,
        &generated_answer,
    ).await?;

    save_logs(...);
}
```

## 10. Answerer の仕様

`longmemeval_kioku_v1` では、Answerer は 1 回だけ実行します。

- best-of-N はしない
- self-consistency はしない
- reranking はしない
- gold answer を answer selection に使わない

Answerer の温度は `0` を原則とします。

### 10.1 Answerer prompt の要求

LongMemEval + KIOKU 用の Answerer prompt は、最低限次を満たす必要があります。

- 与えられた memory context だけに基づいて答える
- 外部知識で補わない
- `question_date` を現在時点として解釈する
- 更新タスクでは最新の状態を優先する
- explanation や chain-of-thought は出さない
- context が不足していると判断したときは、決め打ちの sentinel を返す

v1 では、insufficient context の sentinel は次で固定します。

- `NOT_ENOUGH_MEMORY`

### 10.2 推奨テンプレート

- template id: `longmemeval.kioku.answer.v1`

system prompt:

```text
You answer questions using only the provided memory context.
Treat the provided current date as the reference time for temporal reasoning.
Prefer the latest updated information when the memory context contains older and newer states.
Do not use external knowledge.
If the memory context is insufficient, answer exactly: NOT_ENOUGH_MEMORY
Return only the final answer. Do not include explanations.
```

user prompt:

```text
Memory context:
{context_text}

Current date:
{question_date}

Question:
{question}
```

## 11. Judge の仕様

`longmemeval_kioku_v1` では judge はすべて LLM judge に寄せます。  
ただし v1 では **2 種類の binary judge** だけを実装します。

1. gold-conditioned retrieval sufficiency judge
2. answer correctness judge

LoCoMo と違うのは、LongMemEval では **question type ごとに rubric を切り替える** 点です。  
official `evaluate_qa.py` の task semantics をここで引き継ぎます。

### 11.1 共通ルール

judge の共通ルールは次です。

- model は config で指定する
- temperature は `0`
- 1 question あたり 1 回だけ判定する
- transport retry は許可する
- parse 不能や API failure は run failure とする
- judge の結果を wrong / insufficient に丸めて握りつぶさない
- prompt には `question_type` と `question_date` を必ず含める

この仕様では、judge の品質は **使用モデル + prompt version** に依存します。  
したがって、比較可能なのは同じ judge 条件で取った run 同士だけです。

## 12. Gold-Conditioned Retrieval Sufficiency Judge

### 12.1 目的

gold-conditioned retrieval sufficiency judge は、  
**「与えられた gold answer を、その retrieval context だけで question type の rubric に従って正当化できるか」**  
を判定します。

ここで見るのは answer の出来ではなく、**gold answer を支える retrieval context の十分性**です。  
つまりこの judge は、context 単体の自律的な十分性ではなく、**gold answer を条件にした十分性**を判定します。

### 12.2 入力

入力は次です。

- question
- gold answers
- question type
- question date
- is_abstention
- retrieved context text

judge は generated answer を見ません。

### 12.3 abstention の扱い

abstention question は gold-conditioned retrieval sufficiency の main score から **除外**します。

理由は次です。

- official retrieval evaluation でも abstention 30 問は除外される
- abstention は「正解 evidence が存在しない」こと自体を問う問題である
- gold-conditioned retrieval sufficiency を 2 値で定義すると、通常 question と意味論がずれやすい

したがって v1 では、retrieval headline metrics の分母は **non-abstention question のみ**とします。

### 12.4 ラベル

ラベルは 2 値です。

- `SUFFICIENT`
- `INSUFFICIENT`

`SUFFICIENT` の定義は次です。

- provided context だけで、gold answer を正当化できる
- provided context だけで、gold answer と意味的に同等な答えを導ける
- 表現の言い換えは許す
- type-specific rubric を満たせるだけの根拠がある

特に次のケースは `INSUFFICIENT` とします。

- `single-session-user` / `single-session-assistant` / `multi-session`
  - 必要情報の一部しかない
- `temporal-reasoning`
  - time anchor や計算に必要な日付が欠けている
- `knowledge-update`
  - 更新前情報しかない、または更新後 state を特定できない
- `single-session-preference`
  - personalization に必要な user preference / profile signal が足りない

### 12.5 出力 JSON

template id: `longmemeval.kioku.judge.retrieval.v1`

ここでいう `retrieval` は、より正確には **gold-conditioned retrieval sufficiency** を指します。  
template id や既存の field 名では `retrieval` / `is_sufficient` を使いますが、意味論は本節の定義に従います。

出力は次の JSON に固定します。

```json
{
  "label": "SUFFICIENT",
  "supported_answer": "blue ceramic mug",
  "reason": "The context contains the user's preference and enough detail to answer the question."
}
```

## 13. Answer Correctness Judge

### 13.1 目的

answer correctness judge は、  
**generated answer が、その question type の rubric に従って gold answer と意味的に一致しているか**  
を判定します。

### 13.2 入力

入力は次です。

- question
- gold answers
- question type
- question date
- is_abstention
- generated answer

judge は retrieval context を見ません。

### 13.3 ラベル

ラベルは 2 値です。

- `CORRECT`
- `WRONG`

判定方針は次です。

- `single-session-user` / `single-session-assistant` / `multi-session`
  - gold を含むか、または同等の答えに到達していれば正解
  - 必要情報の一部だけでは不正解
- `temporal-reasoning`
  - 日数 / 週数 / 月数の off-by-one は許容
- `knowledge-update`
  - 古い情報を含んでも、最終的に更新後の正しい答えが出ていれば正解
- `single-session-preference`
  - rubric の全項目は不要
  - user の personal information を正しく使えていれば正解
- abstention
  - 「答えられない」「情報が足りない」と正しく判断できれば正解

`NOT_ENOUGH_MEMORY` は通常 question では通常不正解です。  
ただし abstention では、それが適切な unanswerable judgement なら正解になりえます。

ただし、この abstention 正答率は **memory backend 単体の性能** というより、

- backend が誤誘導する context を出していないか
- answer prompt が「根拠がなければ答えない」を十分に伝えているか
- answerer LLM が不足時に hallucinate せず抑制できるか

を合わせた **system-level の挙動** を強く反映します。  
そのため v1 では、abstention answer accuracy は main backend score とは別枠で報告します。

### 13.4 出力 JSON

template id: `longmemeval.kioku.judge.answer.v1`

```json
{
  "label": "CORRECT",
  "reason": "The generated answer matches the gold answer under the knowledge-update rubric."
}
```

## 14. v1 の headline metrics

`longmemeval_kioku_v1` の headline metrics は次の 5 つです。

1. `overall_answer_accuracy`
2. `task_averaged_answer_accuracy`
3. `abstention_answer_accuracy`
4. `overall_retrieval_sufficiency_accuracy`
5. `task_averaged_retrieval_sufficiency_accuracy`

ここで `overall_answer_accuracy` と `task_averaged_answer_accuracy` は、**通常 question (non-abstention) のみ** を対象とする main backend score です。  
`abstention_answer_accuracy` は別枠の system-level 指標です。

これだけで、まずは次を判断できます。

- KIOKU の retrieval が十分だったか
- retrieval が十分でも最終 answer で落ちていないか
- 特定 type だけが改善しているのか、全体に効いているのか
- abstention が壊れていないか

### 14.1 overall の定義

overall は次の pooled micro average とします。

- `overall_answer_accuracy`
  - `CORRECT` と判定された non-abstention question 数 / 全 non-abstention question 数
- `overall_retrieval_sufficiency_accuracy`
  - `SUFFICIENT` と判定された non-abstention question 数 / 全 non-abstention question 数
- `abstention_answer_accuracy`
  - `CORRECT` と判定された abstention question 数 / 全 abstention question 数

したがって v1 では、answer / retrieval の main backend comparison は **どちらも non-abstention を分母に取る** ものとします。

### 14.2 task-averaged の定義

task-averaged は type ごとの accuracy の単純平均です。

- `task_averaged_answer_accuracy`
  - 6 type の **non-abstention answer accuracy** の macro average
- `task_averaged_retrieval_sufficiency_accuracy`
  - 6 type の **non-abstention retrieval sufficiency accuracy** の macro average

type 分布に偏りがあるため、LongMemEval では overall だけでなく task-averaged も headline に含めます。

### 14.3 per-type

headline は上記 5 指標ですが、レポートには次も出します。

- type ごとの non-abstention answer accuracy
- type ごとの non-abstention retrieval sufficiency accuracy

つまり、`per_type_*` は **question_type ごとの通常 question** を集計したものです。  
abstention は各 type に混在しうるものの、`per_type_*` と `task_averaged_*` の分母には入れません。

### 14.4 補助カウント

v1 では次の補助値も出します。

- `question_count`
- `non_abstention_question_count`
- `abstention_question_count`
- `average_context_token_count`

`average_context_token_count` は、KIOKU backend が 1 question あたり Answerer に渡した
`prompt_context.text` の平均 token 数です。  
ここでの token 数は、その run で固定した tokenizer を用いて
`prompt_context.text` のみを tokenize して算出します。

この値には次を含めません。

- question
- answer prompt の system prompt
- judge prompt
- generated answer

これは backend が最終的に提示した memory context の大きさを表す補助指標であり、  
backend 間の retrieval cost / context efficiency の proxy として使います。

## 15. v1 で出力するファイル

`longmemeval_kioku_v1` では、少なくとも次を出力します。

- `answers.jsonl`
- `retrieval.jsonl`
- `metrics.json`
- `run.resolved.json`

## 16. `answers.jsonl` の仕様

1 line = 1 question です。

最低限次を持ちます。

```json
{
  "dataset": "longmemeval",
  "case_id": "longmemeval:q123",
  "question_id": "longmemeval:q123",
  "question": "What mug does the user prefer?",
  "generated_answer": "blue ceramic mug",
  "gold_answers": ["blue ceramic mug"],
  "is_correct": true,
  "score": 1.0,
  "label": "CORRECT",
  "question_type": "single-session-preference",
  "category": null,
  "is_abstention": false,
  "answer_metadata": {
    "template_id": "longmemeval.kioku.answer.v1",
    "answerer_model": "..."
  },
  "judgement_metadata": {
    "judge_kind": "longmemeval_kioku_answer_llm",
    "judge_model": "...",
    "judge_prompt_id": "longmemeval.kioku.judge.answer.v1",
    "reason": "The generated answer matches the gold answer under the preference rubric."
  }
}
```

## 17. `retrieval.jsonl` の仕様

1 line = 1 question です。

最低限次を持ちます。

```json
{
  "dataset": "longmemeval",
  "case_id": "longmemeval:q123",
  "question_id": "longmemeval:q123",
  "question_type": "single-session-preference",
  "context_kind": "history-chats-with-facts",
  "context_text": "1. [fact] The user prefers blue ceramic mugs.\n2. [support] ...",
  "is_sufficient": true,
  "score": 1.0,
  "label": "SUFFICIENT",
  "judge_metadata": {
    "judge_kind": "longmemeval_kioku_retrieval_llm",
    "judge_model": "...",
    "judge_prompt_id": "longmemeval.kioku.judge.retrieval.v1",
    "supported_answer": "blue ceramic mug",
    "reason": "The context contains the preference and enough support to answer."
  },
  "evidence_event_ids": [
    "longmemeval:q123:s7:t2"
  ],
  "evidence_session_ids": [
    "s7"
  ],
  "metadata": {
    "backend": "kioku"
  }
}
```

abstention question では `is_sufficient`, `score`, `label` を `null` にしてよいです。  
その場合も retrieval log 自体は残します。

## 18. `metrics.json` の仕様

`metrics.json` は protocol 固有の provenance を明示する必要があります。

最低限次を持ちます。

```json
{
  "dataset": "longmemeval",
  "protocol": "longmemeval_kioku_v1",
  "answer_judge_kind": "longmemeval_kioku_answer_llm",
  "retrieval_judge_kind": "longmemeval_kioku_retrieval_llm",
  "metric_semantics_version": "longmemeval_kioku_v1",
  "provisional": false,
  "context_tokenizer": "cl100k_base",
  "context_token_count_scope": "prompt_context_text_only",
  "answer_judge_model": "...",
  "retrieval_judge_model": "...",
  "answer_judge_prompt_id": "longmemeval.kioku.judge.answer.v1",
  "retrieval_judge_prompt_id": "longmemeval.kioku.judge.retrieval.v1",
  "answerer_model": "...",
  "metrics": {
    "question_count": 500,
    "non_abstention_question_count": 470,
    "abstention_question_count": 30,
    "overall_answer_accuracy": 0.81,
    "task_averaged_answer_accuracy": 0.79,
    "abstention_answer_accuracy": 0.90,
    "overall_retrieval_sufficiency_accuracy": 0.84,
    "task_averaged_retrieval_sufficiency_accuracy": 0.82,
    "average_context_token_count": 137.4,
    "per_type_answer_accuracy": {
      "single-session-user": { "correct": 65, "total": 70, "accuracy": 0.9286 },
      "single-session-assistant": { "correct": 47, "total": 56, "accuracy": 0.8393 },
      "single-session-preference": { "correct": 55, "total": 60, "accuracy": 0.9167 },
      "multi-session": { "correct": 72, "total": 95, "accuracy": 0.7579 },
      "knowledge-update": { "correct": 66, "total": 78, "accuracy": 0.8462 },
      "temporal-reasoning": { "correct": 58, "total": 111, "accuracy": 0.5225 }
    },
    "per_type_retrieval_sufficiency_accuracy": {
      "single-session-user": { "correct": 67, "total": 70, "accuracy": 0.9571 },
      "single-session-assistant": { "correct": 50, "total": 56, "accuracy": 0.8929 },
      "single-session-preference": { "correct": 56, "total": 60, "accuracy": 0.9333 },
      "multi-session": { "correct": 78, "total": 95, "accuracy": 0.8211 },
      "knowledge-update": { "correct": 71, "total": 78, "accuracy": 0.9103 },
      "temporal-reasoning": { "correct": 61, "total": 111, "accuracy": 0.5495 }
    }
  }
}
```

## 19. config の最小追加仕様

現在の `crates/evaluate` では legacy な `[prompt.longmemeval]` は廃止されており、
LongMemEval 実行は `[benchmark.longmemeval]` と `[judge]` を前提にします。

また、CLI から実行できる backend は現時点では `return-all` のみで、  
`backend.kind = "kioku"` はまだ未実装です。

推奨 config は次です。

```toml
[run]
input = "..."
output_dir = "..."

[backend]
kind = "return-all"

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

[benchmark.longmemeval]
answer_template_id = "longmemeval.kioku.answer.v1"
answer_judge_prompt_id = "longmemeval.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "longmemeval.kioku.judge.retrieval.v1"
```

古い設定ファイルの `[prompt.longmemeval]` は parse error になります。
LongMemEval 実行時の benchmark 設定は `[benchmark.longmemeval]` に一本化されています。

## 20. current codebase の実装状態

現在の `crates/evaluate` では、LongMemEval path は `longmemeval_kioku_v1` 前提の実装へ移行済みです。  
以下は現状コードの要点です。

### 20.1 LongMemEval path は protocol-specific runner へ分離済み

LongMemEval は共通 `EvaluatePipeline` ではなく、  
`LongMemEvalKiokuEvaluatePipeline` で実行されます。

旧 `LongMemEvalJudge` は削除されており、  
暫定 exact match path は current codebase には残っていません。

### 20.2 Judge を 2 系統にする

LongMemEval では、LoCoMo と同様に次の 2 系統 judge が使われます。

- `RetrievalJudge`
- `AnswerJudge`

current codebase では、LongMemEval 用に次の実装が入っています。

- `LongMemEvalKiokuRetrievalJudge`
- `LongMemEvalKiokuAnswerJudge`

どちらも question type rubric と abstention 指示を prompt に含める実装です。

### 20.3 LongMemEval 実行時に `prompt_context` を必須化する

LongMemEval 実行では `prompt_context` は型として必須です。  
runner が optional fallback を持つのではなく、backend adapter が必ず
evaluation-ready な context を返します。

### 20.4 retrieval log schema を `prompt_context` 中心へ整理する

current `retrieval.jsonl` には、少なくとも次が出力されます。

- `context_kind`
- `context_text`
- `is_sufficient`
- `score`
- `label`
- `judge_metadata`

一方で、現状コードでは `question_type` と `context_token_count` は  
`retrieval.jsonl` には保持していません。

また、abstention question でも retrieval judge は実行され、  
`is_sufficient` / `score` / `label` は `null` ではなく実値で記録されます。

### 20.5 metrics report schema を拡張する

current `metrics.json` では、LongMemEval 用に少なくとも次が出力されます。

- `non_abstention_question_count`
- `abstention_question_count`
- `overall_answer_accuracy`
- `overall_retrieval_sufficiency_accuracy`
- `abstention_answer_accuracy`
- `task_averaged_answer_accuracy`
- `task_averaged_retrieval_sufficiency_accuracy`
- `average_context_token_count`
- `per_type_answer_accuracy`
- `per_type_retrieval_sufficiency_accuracy`

main score の answer / retrieval 指標と per-type 指標は  
non-abstention question のみを分母に集計します。

また、`prompt_context.text` を tokenize して得た context サイズ比較のため、  
current codebase では provenance に次を保持します。

- `context_tokenizer`

tokenizer 名は現状 `whitespace_v1` に固定です。  
`context_token_count_scope` は current schema にはありません。

加えて、`MetricsReport` の provenance は current codebase でも  
top-level flatten のままです。`metrics.provenance` object にはネストされません。

### 20.6 retrieval log に context token 数を追加する

`prompt_context.text` の token 数は current codebase では  
run 全体の aggregate として `metrics.json` にのみ保持されます。  
question ごとの `context_token_count` は `retrieval.jsonl` には出力されません。

### 20.7 config schema を拡張する

`PromptConfig` には `longmemeval_kioku` が追加済みで、  
LongMemEval の prompt 設定はこの section のみを使います。

## 21. v1 で採らないもの

意図的に採らないものを再掲します。

- official retrieval recall / nDCG の headline 化
- official leaderboard への厳密追従
- judge 3 runs
- best-of-N
- human eval
- grounding / faithfulness judge
- `M-cleaned` の main score 化
