# Phase 3 実装計画

## 1. 目的

Phase 3 の目的は、Phase 2 で導入した暫定 prompt builder を見直し、**LoCoMo と LongMemEval の benchmark 仕様に沿って回答用 prompt を構築する仕組みを `Answerer` から分離して実装すること** です。

この Phase では judge の official rubric への準拠までは行いません。  
まずは次を成立させます。

1. 回答用 prompt の構築責務を `Answerer` から切り離せる
2. LoCoMo の category 1-4 / category 5 で別 template を選べる
3. LongMemEval の回答用 prompt を `question_type` ではなく context profile で切り替えられる
4. `DebugAnswerer` と LLM 系 answerer が同じ prompt 構築結果を受け取れる
5. 選ばれた prompt template / profile を answer log に残せる

## 2. Phase 3 の完了条件

Phase 3 の完了条件は次です。

1. `PromptBuilder` あるいは同等の回答 prompt 構築抽象が定義されている
2. `PreparedPrompt` 相当の構造体が定義されている
3. runner が retrieval 後に prompt を構築し、その結果を `Answerer` へ渡す形に整理されている
4. LoCoMo の official answer template を category 1-4 と category 5 で切り替えられる
5. LongMemEval の answer template を `no-retrieval` / `history chats` / `history chats + facts` / `facts only` と `cot` の有無で切り替えられる
6. LongMemEval の `Current Date` を prompt に埋め込める
7. `RunConfig` に LongMemEval 用の回答 prompt profile 設定が追加され、既定値なしの必須項目として扱われる
8. backend が要求された LongMemEval prompt profile を満たせない場合は fail-fast にエラーになる
9. `DebugAnswerer` と `LlmBackedAnswerer` が同じ `PreparedPrompt` を受け取る
10. `GeneratedAnswer.metadata.prompt.{template_id, requested_profile, resolved_profile, context_kind}` を `answers.jsonl` に残せる
11. LoCoMo / LongMemEval それぞれの prompt selection と整形に unit test がある
12. facts 系 profile (`history chats + facts`, `facts only`) は PromptBuilder の unit test で template selection を保証し、backend 未対応時は integration で fail-fast を保証する

## 3. 前提整理

### 3.1 Phase 2 の prompt builder は暫定実装

Phase 2 では、LLM Answerer を接続するために prompt builder を導入しました。  
しかし現状の構成では、回答 prompt の構築責務が `answerer` モジュール配下にあり、`LlmBackedAnswerer` が直接 prompt を組み立てています。

この状態だと次の問題があります。

- prompt 構築が benchmark/profile の責務ではなく LLM 実装の都合に見える
- `DebugAnswerer` は同じ prompt 構築結果を共有していない
- LoCoMo / LongMemEval の dataset-specific な template 選択を拡張しにくい
- LongMemEval の回答 prompt と judge prompt の責務が混ざりやすい

### 3.2 LongMemEval の回答 prompt と judge prompt は分けて考える

LongMemEval では、回答用 prompt と判定用 prompt の分岐キーが異なります。

- 回答用 prompt
  - `no-retrieval`
  - `history chats`
  - `history chats + facts`
  - `facts only`
  - `cot`
  - `Current Date`
- 判定用 prompt
  - `question_type`
  - `is_abstention`

このため、Phase 3 では **回答用 prompt のみ** を扱います。  
LongMemEval の official anscheck prompt や type-specific rubric は Phase 4 で judge 側に実装します。

### 3.3 PromptBuilder は backend と Answerer の間に置く

責務の分け方は次に固定します。

- backend
  - `RetrievedMemory` や必要なら prompt-ready な context を返す
  - 要求された LongMemEval prompt profile を満たせるか検証する
- PromptBuilder
  - benchmark/profile に応じて template を選び、prompt を構築する
- Answerer
  - 構築済み prompt を実行して `GeneratedAnswer` を返す

### 3.4 LongMemEval の prompt profile は RunConfig で明示指定する

LongMemEval の prompt profile は backend の能力に依存しますが、評価条件としても再現可能である必要があります。  
そのため、Phase 3 では **RunConfig に LongMemEval 用 prompt profile を必須設定として持たせる** 方針を採ります。

- prompt profile の要求値は runner が `RunConfig` から受け取る
- backend は、その要求値に対応する `PromptContext` を返せるかを判定する
- 満たせない場合は暗黙 fallback せずエラーにする

これにより、評価条件の再現性と backend capability の検証を両立します。

## 4. 設計原則

### 4.1 prompt 構築は benchmark logic

prompt 構築は `debug` / `openai-compatible` のような answerer 実装差分ではなく、LoCoMo / LongMemEval の benchmark 差分です。  
したがって `answerer` 実装の内部責務にはしません。

### 4.2 template selection と context rendering を分ける

PromptBuilder では、少なくとも次の 2 段を分けて扱います。

- template selection
  - どの文面を使うか
- context rendering
  - retrieved result をどう prompt に埋め込むか

LongMemEval では、history chats と facts only では context の見せ方自体が異なるため、この分離が必要です。

### 4.3 backend が prompt-ready context を返せる余地を残す

LongMemEval の `history chats + facts` や `facts only` を正しく扱うには、`RetrievedMemory` の共通整形だけでは足りない可能性があります。  
将来の backend が独自に整形した context を返せるよう、PromptBuilder は `RetrievedMemory` だけに依存しない設計を採ります。

## 5. コア I/F 設計

### 5.1 `PreparedPrompt`

回答用 prompt の共通表現を導入します。

```rust
pub struct PreparedPrompt {
    pub system_prompt: Option<String>,
    pub user_prompt: String,
    pub template_id: String,
    pub metadata: serde_json::Value,
}
```

`template_id` はログと検証用です。  
例:

- `locomo.qa.default.v1`
- `locomo.qa.cat5.v1`
- `longmemeval.answer.history_chats.v1`
- `longmemeval.answer.facts_only.cot.v1`

### 5.2 `PromptContext`

retrieval 結果をそのまま列挙するだけでなく、backend が prompt-ready な context を返せるようにします。

```rust
pub enum PromptContextKind {
    RetrievedMemories,
    NoRetrieval,
    HistoryChats,
    HistoryChatsWithFacts,
    FactsOnly,
}

pub struct PromptContext {
    pub kind: PromptContextKind,
    pub text: String,
    pub metadata: serde_json::Value,
}
```

`PromptContext` は backend から runner へ `QueryOutput` 経由で渡します。

```rust
pub struct QueryOutput {
    pub retrieved: Vec<RetrievedMemory>,
    pub prompt_context: Option<PromptContext>,
    pub metadata: serde_json::Value,
}
```

Phase 3 では `ReturnAllMemoryBackend` は最低限 `RetrievedMemories` もしくは `HistoryChats` を返せれば十分です。  
`HistoryChatsWithFacts` / `FactsOnly` は、将来の backend 追加や手動 fixture で拡張できるよう型だけ先に固定します。

したがって、facts 系 profile は Phase 3 の時点では **PromptBuilder 側で template 選択と prompt 整形を unit test で固定する対象** であり、`ReturnAllMemoryBackend` で end-to-end 実行可能にすることまでは完了条件に含めません。  
`ReturnAllMemoryBackend` に facts 系 profile を要求した場合は、backend capability 不足として fail-fast にエラーとします。

LongMemEval では `RunConfig` で要求された prompt profile に応じて、backend が適切な `PromptContext` を返すか、返せなければエラーにします。

### 5.3 `PromptBuilder`

```rust
pub trait PromptBuilder {
    fn build_answer_prompt(
        &self,
        request: PromptBuildRequest<'_>,
    ) -> anyhow::Result<PreparedPrompt>;
}
```

`PromptBuildRequest` には次を含めます。

- `dataset`
- `case`
- `question`
- `retrieved`
- `prompt_context`
- `requested_prompt_profile`

`prompt_context` がない場合は、PromptBuilder か補助 renderer が `retrieved` から最低限の文脈を整形します。

### 5.4 `RunConfig` に LongMemEval prompt profile を追加する

LongMemEval 用の回答 prompt profile は設定ファイルから明示的に受け取ります。  
Phase 3 では既定値を持たせず、LongMemEval 実行時は必須項目として扱います。

例:

```toml
[prompt.longmemeval]
answer_profile = "history-chats"
cot = false
```

`answer_profile` の候補は次です。

- `no-retrieval`
- `history-chats`
- `history-chats-with-facts`
- `facts-only`

LoCoMo 実行時はこの設定を参照しません。  
LongMemEval 実行時に未指定なら validation error にします。

## 6. dataset ごとの実装方針

### 6.1 LoCoMo

LoCoMo では official answer template をそのまま使います。

- category 1-4
  - `Based on the above context, write an answer in the form of a short phrase ...`
- category 5
  - `Based on the above context, answer the following question.`

LoCoMo では `question_type` による回答 template 分岐は不要です。  
prompt selection のキーは `category` です。

以下は LoCoMo 公式のプロンプトテンプレートです。

```python
QA_PROMPT = """
Based on the above context, write an answer in the form of a short phrase for the following question. Answer with exact words from the context whenever possible.

Question: {} Short answer:
"""

QA_PROMPT_CAT_5 = """
Based on the above context, answer the following question.

Question: {} Short answer:
"""
```

### 6.2 LongMemEval

LongMemEval の回答 template は `question_type` ではなく、**context profile** で切り替えます。

最初に対応する profile は次です。

- `no-retrieval`
- `history chats`
- `history chats + facts`
- `facts only`
- 各 profile に対する `cot = true | false`

これらの profile は dataset 側から自動推論せず、`RunConfig` で明示指定された値を使います。

また、LongMemEval では `Current Date: {}` を prompt に含めるので、質問日時を人間可読な形で取り出せる必要があります。

対応方針は次です。

1. `BenchmarkQuestion` には、記憶システム向けに整形した `question_date` を保持する
2. `BenchmarkQuestion.metadata` には、prompt 再現用の raw datetime string を保持する
3. Answerer 用 prompt では raw datetime string を優先して `Current Date` に使う

評価データセット由来の表記を保った方が、prompt 再現性の観点で扱いやすいためです。  
一方で、記憶システム側は都合に応じて別フォーマットの日時文字列を必要とする可能性があるため、dataset adapter では raw の datetime string を失わずに保持します。

LongMemEval 公式のプロンプト構築関数は [longmemeval_prompt.py](longmemeval_prompt.py) です。

なお、backend が要求された profile に対応する `PromptContext` を返せない場合は、別 profile へ暗黙に切り替えずエラーとします。

## 7. runner / Answerer の変更方針

### 7.1 runner

`EvaluatePipeline` の question 処理順を次へ変更します。

1. backend `query`
2. retrieval log を記録
3. `RunConfig` の prompt profile をもとに backend / prompt 用 context を検証する
4. `PromptBuilder` で `PreparedPrompt` を構築
5. `PreparedPrompt` を含む request を `Answerer` へ渡す
6. `Judge` で採点

### 7.2 Answerer

`Answerer` は benchmark-specific な prompt template 選択を行いません。  
代わりに `PreparedPrompt` を受け取り、それをどう実行するかだけを担当します。

- `LlmBackedAnswerer`
  - `PreparedPrompt` を `LlmGenerateRequest` に変換して実行
- `DebugAnswerer`
  - 固定応答を返してよい
  - ただし prompt metadata を `GeneratedAnswer.metadata.prompt.*` に転記する

`GeneratedAnswer.metadata` の schema は namespaced に固定します。  
少なくとも次を想定します。

```json
{
  "prompt": {
    "template_id": "longmemeval.answer.history_chats.v1",
    "requested_profile": "history-chats",
    "resolved_profile": "history-chats",
    "context_kind": "HistoryChats"
  }
}
```

answerer 固有 metadata や LLM 応答 metadata は、必要に応じて `answerer.*` / `llm.*` のような別 namespace に分けて保持します。

## 8. モジュール構成

Phase 3 では prompt 関連を `answerer/` から切り離します。

```text
crates/evaluate/src/
├── prompt/
│   ├── mod.rs
│   ├── answer.rs
│   ├── context.rs
│   └── profiles/
│       ├── locomo.rs
│       └── longmemeval.rs
├── answerer/
│   ├── mod.rs
│   ├── traits.rs
│   ├── debug.rs
│   ├── llm.rs
│   └── rig_openai.rs
```

Phase 2 の `answerer/prompt.rs` は、Phase 3 で `prompt/` 配下へ移すか、薄い互換 wrapper に縮小します。

## 9. 実装ステップ

1. `PreparedPrompt` と `PromptContext` の型を定義する
2. `PromptBuilder` I/F を定義する
3. `RunConfig` に LongMemEval prompt profile と `cot` の必須設定を追加する
4. LongMemEval 実行時の config validation を実装する
5. LoCoMo 用 prompt profile を実装する
6. LongMemEval 用 prompt profile を実装する
7. LongMemEval の raw datetime string を prompt 再現用 metadata に保持し、記憶システム向け `question_date` と分けて扱う
8. runner で prompt 構築を行うよう処理順を変更する
9. `Answerer` が `PreparedPrompt` を受け取る API に寄せる
10. `DebugAnswerer` と `LlmBackedAnswerer` を新 I/F に合わせて更新する
11. `answers.jsonl` に `GeneratedAnswer.metadata.prompt.*` を出す

## 10. テスト計画

Phase 3 で最低限入れるテストは次です。

1. LoCoMo category 1-4 で default template が選ばれる test
2. LoCoMo category 5 で cat5 template が選ばれる test
3. LongMemEval `NoRetrieval` profile の template selection test
4. LongMemEval `HistoryChats` profile の template selection test
5. LongMemEval `HistoryChatsWithFacts` profile の template selection test
6. LongMemEval `FactsOnly` profile の template selection test
7. LongMemEval `cot = true` で step-by-step 文面になる test
8. LongMemEval prompt に `Current Date` が入る test
9. LongMemEval 実行時に prompt profile 未指定だと config validation error になる test
10. `ReturnAllMemoryBackend` で facts 系 profile を要求した場合に fail-fast する test
11. runner が prompt 構築後に `Answerer` を呼ぶ test
12. `DebugAnswerer` でも prompt metadata が answer log に残る test

## 11. 非目標

Phase 3 では次はまだ行いません。

- LoCoMo の official judge / F1 ベース採点
- LongMemEval の type-specific anscheck prompt
- LongMemEval の `question_type` ごとの judge rubric
- retrieval 指標の追加
- `KiokuMemoryBackend` 実装

これらは Phase 4 以降で扱います。

## 12. リスクと対策

### 12.1 context profile の責務が曖昧になるリスク

対策:

- template selection と context rendering を別型に分ける
- `PromptContextKind` を導入して意味を明示する

### 12.2 LongMemEval の回答 prompt と judge prompt が再び混ざるリスク

対策:

- Phase 3 は回答 prompt のみを扱うと文書とモジュールで明示する
- judge prompt は Phase 4 の責務として `judge/` 側に切り出す

### 12.3 backend ごとの差分を PromptBuilder が抱え込み過ぎるリスク

対策:

- backend が返せるなら `PromptContext` を優先して使う
- `PromptBuilder` は template 選択に集中し、backend 固有の fact 抽出までは持たない

### 12.4 prompt profile 指定ミスに気づきにくいリスク

対策:

- LongMemEval の prompt profile は既定値を持たせず設定必須にする
- backend が要求 profile を満たせない場合は暗黙 fallback せずエラーにする
