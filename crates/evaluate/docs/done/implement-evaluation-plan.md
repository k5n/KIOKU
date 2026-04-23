# 評価プログラムの実装方針

## 1. 目的

この評価プログラムの目的は、**記憶層の実装と無関係に同じ I/F で評価できる基盤** を先に作ることです。

そのため、設計上は次の分離を明確にします。

- データセットの読み込み
- ベンチマーク用イベント列への変換
- 記憶層への `ingest` / `query`
- 回答生成
- 採点
- 集計とレポート

ここでの当面の完成条件は次です。

1. LoCoMo と LongMemEval を読み込める
2. 共通 runner で両方回せる
3. スタブ記憶層で動く
4. PromptBuilder と Answerer が分離されている
5. LoCoMo は KIOKU 用仕様 `locomo_kioku_v1` で採点できる

LongMemEval については、まだ KIOKU 用の評価仕様を確定していません。  
そのため、LongMemEval の benchmark-specific judge / retrieval 指標は、仕様確定後に runner へ組み込みます。

## 2. 設計原則

### 2.1 記憶層は Answerer を持たない

比較したいのは「何を覚え、何を返したか」です。  
そのため記憶層自身は最終回答を返さず、**返すのは retrieved memory だけ** にします。

最終回答は別の `Answerer` が作ります。

### 2.2 Judge も別コンポーネントにする

Judge は Answerer とも記憶層とも独立させます。  
これにより、同じ検索結果に対して別 judge を試したり、LongMemEval だけ official judge を使うことができます。

### 2.3 データセット差分は Adapter に閉じ込める

LoCoMo と LongMemEval の差分は runner に持ち込まず、**dataset adapter が共通 case に落とし込む** 方が保守しやすいです。

### 2.4 gold evidence を最後まで保持する

retrieval 指標を出すには、message / session の source ID を途中で落としてはいけません。

- LoCoMo: `dia_id`, `evidence`
- LongMemEval: `session_id`, `answer_session_ids`, `has_answer`

これらは共通 case に乗せて最後まで保持します。

### 2.5 prompt 構築は Answerer とは別責務にする

回答 prompt の構築は、LLM をどう呼ぶかとは別の責務です。

- `Answerer`
  - 構築済み prompt を消費して最終回答を返す
  - `debug`, `openai-compatible` など実行手段の差分を吸収する
- `PromptBuilder`
  - dataset / category / context profile から回答用 template を選ぶ
  - LoCoMo / LongMemEval の benchmark 差分を吸収する

特に LongMemEval では、回答用 prompt と判定用 prompt で分岐キーが異なります。  
回答用 prompt は `question_type` ではなく、`no-retrieval` / `history chats` / `facts only` / `cot` / `current date` などの **文脈提示プロファイル** で切り替える必要があります。  
そのため、prompt 構築は `Answerer` 実装の内側に閉じ込めず、評価プログラム側の benchmark/profile ロジックとして独立させます。

## 3. 推奨アーキテクチャ

ここで重要なのは、`kioku` 本体のドメイン設計と、評価プログラムが都合上ほしい I/F を分離することです。  
`crates/core` は「色々ある記憶層の共通 trait 置き場」ではなく、**KIOKU という今回作る記憶層システム自体のドメイン層** を実装する場所です。

一方で、評価プログラムが必要とする `reset / ingest / query` のような I/F は、**評価プログラムの runner が都合よく回るための I/F** です。これは `crates/evaluate` 側に持たせるのが自然です。

### 3.1 crate の責務

- `crates/core`
  - KIOKU のドメイン層
  - KIOKU が前提とするユースケース、エンティティ、値オブジェクト、ドメイン service
  - 永続化や索引化に必要なポート I/F trait
- `crates/adapters/*`
  - `crates/core` が定義したポートの具体的なインフラ実装
  - 例:
    - 記憶グラフの保存先として SQLite を使う実装
    - ベクトル検索基盤として LanceDB を使う実装
  - 目的は、KIOKU が利用するインフラ層を差し替え可能にすること
- `crates/evaluate`
  - 評価プログラム専用の `MemoryBackend` trait
  - スタブ実装
  - 将来の `KiokuMemoryBackend`
  - dataset adapter
  - runner
  - answerer
  - judge
  - metrics / report

つまり依存の向きは次です。

1. `crates/core` は KIOKU 本体の設計として進める
2. `crates/adapters/*` はそのインフラ実装として追加する
3. `crates/evaluate` は評価用 I/F を独自に持つ
4. 将来、`crates/evaluate` 内で `KiokuMemoryBackend` を実装し、KIOKU を評価 runner に接続する

評価器用 trait を `crates/core` に置いてしまうと、`core` が評価都合の API に引っ張られます。  
それは KIOKU 本体のドメイン境界を曖昧にするので避けた方がよいです。

## 4. 評価プログラム側の I/F の最小形

必要なのは「時系列追加」と「検索」です。  
評価 runner はイベント列を時刻順に流し込めればよく、内部のバックグラウンド処理をどう同期させるかは backend 側の責務とします。

```rust
use async_trait::async_trait;

#[async_trait]
pub trait MemoryBackend {
    async fn reset(&mut self, scope: EvalScope) -> anyhow::Result<()>;
    async fn ingest(&mut self, event: MemoryEvent) -> anyhow::Result<()>;
    async fn query(&mut self, input: QueryInput) -> anyhow::Result<QueryOutput>;
}
```

これは KIOKU 全体の標準 I/F ではなく、**評価 runner がバックエンドを差し替えるための trait** です。  
したがって定義場所は `crates/evaluate` が適切です。

### 4.1 `reset`

ケースごとに記憶状態を初期化します。

- LoCoMo: 1 conversation sample ごと
- LongMemEval: 1 question entry ごと

### 4.2 `ingest`

会話イベントを 1 件ずつ追加します。

```rust
pub struct MemoryEvent {
    pub event_id: String,
    pub timestamp: Timestamp,
    pub location: EventLocation,
    pub speaker_id: String,
    pub speaker_name: String,
    pub content: String,
    pub metadata: serde_json::Value,
}

pub struct EventLocation {
    pub stream_id: String,
    pub parent_stream_id: Option<String>,
    pub metadata: serde_json::Value,
}
```

ここで重要なのは、`MemoryEvent` が持つのは **評価データセット上での「どこで発言されたか」** を表す情報だけで十分だという点です。  
ケース全体の識別は `BenchmarkCase.case_id` や `QueryScope.case_id` が担い、`location` には case の中での stream 情報だけを載せます。  
LoCoMo や LongMemEval が持っている conversation / session / entry といった識別子を必要な粒度で `location` に保持し、KIOKU 側の `space / room / thread` にどう対応付けるかは `KiokuMemoryBackend` が決めます。

### 4.3 バックグラウンド処理のシミュレーション

既存の `doc/implementation-plan.md` では、ベンチマーク用に「バックグラウンド処理が終わったら次へ進める」ためのコールバックを想定していました。  
ただしその仕組みは評価 runner の共通 I/F に露出させず、**`KiokuMemoryBackend` の内部で吸収する** 方が責務分離として自然です。

想定する動作は次です。

- runner は event を時系列順に `ingest` する
- `KiokuMemoryBackend` は、今回の event の timestamp と前回 event の timestamp の差分を使って、KIOKU 側の非同期処理が進んだものとして扱う
- あるいは KIOKU 側のバックグラウンド処理完了コールバックを内部で待つ
- どちらの場合も、同期条件を満たしたら `ingest` を返す

つまり、**評価 runner は「1 件 ingest したら、その event 時点まで backend が整合した」とみなせればよい** ので、`flush(now)` のような外部 API は不要です。  
スタブ実装では `ingest` を単純に push して即 return すれば十分です。`ReturnAllMemoryBackend` もこの方針に従い、ingest 順を保持したまま扱います。

### 4.4 `query`

```rust
pub struct RetrievalBudget {
    pub max_items: Option<usize>,
    pub max_tokens: Option<usize>,
}

pub struct QueryInput {
    pub query_id: String,
    pub timestamp: Timestamp,
    pub scope: QueryScope,
    pub question: String,
    pub budget: RetrievalBudget,
}

pub struct QueryScope {
    pub case_id: String,
    pub stream_ids: Vec<String>,
    pub metadata: serde_json::Value,
}

pub struct QueryOutput {
    pub prompt_context: PromptContext,
    pub metadata: serde_json::Value,
}
```

`QueryInput.timestamp` は質問時点の参照時刻を表すためのものであり、ingest 済み event の cutoff 時刻としては扱いません。
`QueryInput.budget` は answerer に渡す retrieval budget を表すためのものであり、backend 固有の検索パラメータを直接露出させません。
Phase 1 の `ReturnAllMemoryBackend` では `max_items` のみを実装し、`max_tokens` は将来の backend 向け予約フィールドとします。Phase 1.5 で導入する TOML 設定ファイルでは `max_tokens` を設定項目として保持できますが、Phase 1 系 backend では未対応のため明示的にエラーを返します。

Phase 5.6 以降、この共通 contract に raw retrieval item 配列は含めません。  
raw retrieval diagnostics が必要な場合は backend-specific metadata に閉じ込めます。

## 5. benchmark adapter 層

データセットごとの差分を吸収するため、`crates/evaluate` 側で benchmark case を定義します。

```rust
pub struct BenchmarkCase {
    pub case_id: String,
    pub dataset: DatasetKind,
    pub events: Vec<MemoryEvent>,
    pub questions: Vec<BenchmarkQuestion>,
}

pub struct BenchmarkQuestion {
    pub question_id: String,
    pub timestamp: Timestamp,
    pub question: String,
    pub answer: String,
    pub question_type: Option<String>,
    pub evidence_event_ids: Vec<String>,
    pub evidence_session_ids: Vec<String>,
    pub is_abstention: bool,
    pub metadata: serde_json::Value,
}
```

各 dataset adapter は raw JSON から `BenchmarkCase` を作るだけに責務を限定します。  
この層では、KIOKU の `space / room / thread` を先回りして決めません。評価データセット上の location 情報を保持するだけに留めます。

このとき `case_id` / `question_id` / `event_id` は adapter 実装者依存にせず、fixture・log・retrieval 指標の結合キーとして使えるよう決定的規則で固定します。

## 6. dataset ごとの case 変換

### 6.1 LoCoMoAdapter

LoCoMo は 1 conversation sample が 1 case です。

- `case_id`: `locomo:{sample_id}`
- `location.stream_id`: `session_1`, `session_2`, ...
- `events`: 全 turn
- `questions`: sample 内の全 QA

追加で必要な処理:

- `question_id` を `locomo:{sample_id}:q{idx}` で生成する
- `event_id` を `locomo:{sample_id}:event:{dia_id}` で生成する
- category 5 なら `answer` ではなく `adversarial_answer` も保持する
- `evidence` の `dia_id` を同じ規則で `evidence_event_ids` に入れる
- `session_X` / `session_X_date_time` の動的キーをパースし、各 session に開始日時とメッセージ列が 1:1 で存在することを検証する
- session は `session_1`, `session_2`, ... の数値順に sort してから扱う
- session timestamp から turn timestamp を決定的に生成する

### 6.2 LongMemEvalAdapter

LongMemEval は 1 question entry が 1 case です。

- `case_id`: `longmemeval:{question_id}`
- `location.stream_id`: 各 `haystack_session_id`
- `events`: その question に含まれる全 turn
- `questions`: 常に 1 件

追加で必要な処理:

- `question_id` を raw の `question_id` を用いて `longmemeval:{question_id}` に正規化する
- `event_id` を `longmemeval:{question_id}:{session_id}:t{turn_idx}` で生成する
- `question_type` をそのまま保持する
- `question_id.ends_with("_abs")` で `is_abstention` を立てる
- `answer_session_ids` を `evidence_session_ids` に入れる
- turn に `has_answer` があれば同じ規則で `evidence_event_ids` にも落とす
- loader で `haystack_dates` / `haystack_session_ids` / `haystack_sessions` の長さ一致を fail-fast で検証する

**重要**: 現在の `LongMemEvalMessage` 型は `has_answer` を落としているため、ここは先に拡張する必要があります。

### 6.3 ID 正規化規則

canonical ID は次で固定します。

- LoCoMo:
  - `case_id`: `locomo:{sample_id}`
  - `question_id`: `locomo:{sample_id}:q{idx}`
  - `event_id`: `locomo:{sample_id}:event:{dia_id}`
- LongMemEval:
  - `case_id`: `longmemeval:{raw_question_id}`
  - `question_id`: `longmemeval:{raw_question_id}`
  - `event_id`: `longmemeval:{raw_question_id}:{session_id}:t{turn_idx}`

`idx` と `turn_idx` は 0-based とし、`evidence_event_ids`、`source_event_id`、`retrieved_event_ids`、`answers.jsonl` の `question_id` はすべてこの規則で正規化した値を使います。

## 7. PromptBuilder, Answerer と Judge

### 7.1 Answer PromptBuilder

回答 prompt の構築は、`Answerer` から分離します。

役割は次です。

- dataset ごとの回答 template を選ぶ
- question / retrieved memory / prompt context から最終 prompt を組み立てる
- template ID や prompt metadata を後段ログへ渡せる形にする

想定する分岐は次です。

- LoCoMo
  - category 1-4: official `QA_PROMPT`
  - category 5: official `QA_PROMPT_CAT_5`
- LongMemEval
  - `no-retrieval`
  - `history chats`
  - `history chats + facts`
  - `facts only`
  - `cot` の有無
  - `Current Date`

ここで重要なのは、LongMemEval の `question_type` は **判定用 rubric の分岐キー** であり、回答用 prompt の第一分岐ではないことです。  
回答用 prompt は、retriever がどのような文脈を返したかという **context profile** で切り替える方が benchmark 仕様に合います。

また、将来の memory backend が prompt-ready な context を返せるように、PromptBuilder は
backend が返す `PromptContext` をそのまま受け取れる設計にします。

### 7.2 Answerer

Answerer は記憶層共通で固定しますが、**benchmark-specific な prompt template の選択責務は持ちません**。

```rust
#[async_trait]
pub trait Answerer {
    async fn answer(
        &self,
        request: PreparedAnswerRequest<'_>,
    ) -> anyhow::Result<GeneratedAnswer>;
}
```

最初に必要なのは 2 種類だけです。

- `LlmAnswerer`: 構築済み prompt を LLM に渡して回答させる
- `DebugAnswerer`: prompt を無視して固定応答を返すか、prompt をそのまま返す

比較実験では、**Answerer の prompt / model はバックエンド間で固定** します。

`DebugAnswerer` が prompt を実際に使わない場合でも、どの prompt profile / template が選ばれたかは metadata として残せるようにします。

### 7.3 Judge

Judge は dataset ごとに分けます。  
ただし、answer judge と retrieval judge は入力の形が異なるため、1 つの trait に無理に寄せません。

```rust
#[async_trait]
pub trait AnswerJudge {
    async fn judge_answer(
        &self,
        question: &BenchmarkQuestion,
        generated: &GeneratedAnswer,
    ) -> anyhow::Result<Judgement>;
}

#[async_trait]
pub trait RetrievalJudge {
    async fn judge_retrieval(
        &self,
        question: &BenchmarkQuestion,
        context: &PromptContext,
    ) -> anyhow::Result<Judgement>;
}
```

同じ judge runtime を共有しても構いませんが、runner から見た責務は分けておいた方が `locomo_kioku_v1` のような仕様に対応しやすいです。

### LoCoMoJudge

- まず実装対象にするのは `locomo_kioku_v1`
- retrieval sufficiency judge と answer correctness judge の 2 本を持つ
- category 1-4 を main score にする
- category 5 は v1 のスコープ外とする
- LoCoMo 公式 F1 互換 judge は必要になった時点で別 mode として追加する

### LongMemEvalJudge

- KIOKU 用評価仕様が確定するまでは benchmark-specific 実装を固定しない
- 仕様確定後に question type / abstention / retrieval の扱いをまとめて実装する

LongMemEval は仕様未確定のまま official judge 互換へ寄せ切らない方がよいです。

## 8. runner の流れ

runner は dataset に依らず次の順で動かします。

1. raw dataset を load
2. dataset adapter で `BenchmarkCase` に変換
3. case ごとに backend を `reset`
4. `events` を順に `ingest`
5. question 時点で `query`
6. 必要なら `PromptContext` を確定する
7. retrieval judge を実行する
8. 回答用 `PromptBuilder` で prompt を構築
9. `Answerer` で最終回答を生成
10. answer judge を実行する
11. retrieval / answer logs を保存する
12. metrics を集計して report を出す

この流れは `EverOS/evaluation` の 4-stage pipeline に近く、理解しやすいです。

## 9. スタブ記憶層の実装

最初に作るべきバックエンドは `ReturnAllMemoryBackend` です。

仕様は単純で十分です。

- `reset`: 内部の `Vec<StoredEvent>` を空にする
- `ingest`: 受け取った `MemoryEvent` を push
- `query`: その case に追加済みの全 event を ingest 順のまま返す

追加で次の性質を持たせると便利です。

- `source_event_id` を必ず返す
- `source_session_id` を必ず返す
- `score` は `None` か固定値
- `budget.max_items` が指定されたら、ingest 済み event の末尾 N 件だけを選び、返却順も ingest 順のまま保つ

このバックエンドは retrieval 能力はありませんが、**I/F と evaluator 全体が正しいかを検証する基準実装** になります。

この実装も評価プログラムの動作確認用なので、`crates/evaluate` 側に置きます。  
`crates/core` や `crates/adapters/*` に置く必要はありません。

## 10. metrics と保存形式

少なくとも次の 3 系統を分けて保存します。

### 10.1 answer logs

`answers.jsonl`

- `case_id`
- `question_id`
- `question_type`
- `gold_answer`
- `generated_answer`
- `judgement`
- `judge_metadata`
- `judge_kind`
- `metric_semantics_version`
- `provisional`
- `answer_metadata`

### 10.2 retrieval logs

`retrieval.jsonl`

- `case_id`
- `question_id`
- `context_kind`
- `context_text`
- `judge_metadata`
- `judge_kind`
- `metric_semantics_version`
- `provisional`

### 10.3 aggregate metrics

`metrics.json`

- overall accuracy
- per-category / per-type accuracy
- task-averaged accuracy
- abstention accuracy
- retrieval metrics
- token / latency stats
- `judge_kind`
- `metric_semantics_version`
- `provisional`

Phase 1 の簡易 judge を使う段階では、同名メトリクスを最終 benchmark metric と混同しないために次を明記します。

- `judge_kind`: 例 `phase1_exact_match`
- `metric_semantics_version`: 例 `phase1-provisional-v1`
- `provisional`: `true`
- `locomo_overall_scope`: `category_1_4`

特に LoCoMo の `overall accuracy` は `category 1-4` のみを分母とし、`category 5` は別枠で扱います。

また、Phase 1.5 以降は実験設定を `answer_metadata` に重複保存せず、run manifest として別ファイル保存します。

- `run.config.toml`
  - ユーザが渡した元の設定ファイル
- `run.resolved.json`
  - 実際に使った解決済み設定
  - 相対パス解決後の `input` / `output_dir`
  - default 適用後の値
  - 選択された `backend.kind` / `answerer.kind`
  - retrieval budget

`GeneratedAnswer.metadata` と `answers.jsonl.answer_metadata` は answerer 固有 metadata のみを保持し、run-level 設定は含めません。

Phase 2 で `openai-compatible` answerer を導入したら、`run.resolved.json` にはさらに次を追加します。

- `model`
- `base_url`
- `temperature`
- `max_output_tokens`
- `timeout_secs`
- `api_key_env`
- `max_retries`
- `retry_backoff_ms`

一方で、answerer 固有 metadata に追加してよいのは回答ごとに変わり得る値だけに限定します。例えば次です。

- `request_id`
- `finish_reason`
- `usage`
- `latency_ms`
- `raw_response`

API key の実値はログへ残しません。

## 11. `crates/evaluate` の推奨モジュール分割

```text
crates/evaluate/src/
├── lib.rs
├── config/
│   ├── mod.rs
│   └── run.rs
├── backend/
│   ├── mod.rs
│   ├── traits.rs
│   ├── return_all.rs
│   ├── oracle.rs
│   └── kioku.rs
├── datasets/
│   ├── mod.rs
│   ├── locomo.rs
│   └── longmemeval.rs
├── benchmark/
│   ├── mod.rs
│   ├── case.rs
│   └── adapter.rs
├── runner/
│   ├── mod.rs
│   └── pipeline.rs
├── prompt/
│   ├── mod.rs
│   ├── answer.rs
│   ├── context.rs
│   ├── judge.rs
│   └── profiles/
│       ├── locomo.rs
│       └── longmemeval.rs
├── answerer/
│   ├── mod.rs
│   ├── debug.rs
│   ├── llm.rs
│   └── rig_openai.rs
├── judge/
│   ├── mod.rs
│   ├── locomo.rs
│   └── longmemeval.rs
├── metrics/
│   ├── mod.rs
│   ├── retrieval.rs
│   └── aggregate.rs
├── report.rs
└── bin/
    └── evaluate.rs
```

今ある `src/bin/locomo.rs` と `src/bin/longmemeval.rs` の型定義は、最終的には `datasets/` に移した方が再利用しやすいです。

`backend/traits.rs` に評価用 `MemoryBackend` を置き、`return_all.rs` や `oracle.rs` にスタブ実装を置きます。  
将来 KIOKU が動くようになったら、`kioku.rs` に `KiokuMemoryBackend` を実装して runner から差し替えます。  
この adapter が、評価データセットの location 情報を KIOKU の `space / room / thread` にマッピングし、必要ならバックグラウンド処理の同期も内部で吸収します。

## 12. 段階的な実装順

Phase 1-3 までは、公式 prompt への寄せを含めてすでに進めた実装方針をそのまま活かします。  
ここで得られた一番重要な成果は、**PromptBuilder と Answerer を分離し、prompt 構築を benchmark/profile ロジックとして独立させたこと**です。  
LoCoMo / LongMemEval の公式 prompt に寄せた部分は最終仕様ではなくなったものの、この分離自体はそのまま有効です。

ここから先は、各 benchmark の「公式互換」を先に目指すのではなく、**KIOKU 用に定めた評価仕様を順番に実装する** 方針に切り替えます。

### Phase 1: stub で runner を完走させる （完了）

まずは最小実装で end-to-end を通します。

1. `LongMemEvalMessage` に `has_answer` を追加
2. LoCoMo / LongMemEval を `BenchmarkCase` へ変換
3. `ReturnAllMemoryBackend` を作る
4. `DebugAnswerer` のような fixed 応答の Answerer を作る
5. 最小の `LoCoMoJudge` と `LongMemEvalJudge` を作る
6. canonical ID と provisional metric semantics を含む logs / metrics を出す
7. overall / per-category / per-type accuracy を出す

この段階で「全履歴を返すだけでベンチマークが最後まで回る」状態にします。  
ここで得られる metrics は、比較用の最終スコアではなく **runner / adapter / logging の配線確認用** と位置付けます。

### Phase 1.5: TOML 設定ファイルへ移行する （完了）

次に、評価実行設定を CLI 引数直書きから TOML 設定ファイルへ移します。

1. `RunConfig` を定義する
2. dataset / input / output_dir / backend / answerer / budget を TOML で読めるようにする
3. CLI は `--config <path>` のみを受けるようにする
4. `api_key_env` を optional な設定項目として持てるようにする
5. `run.config.toml` と `run.resolved.json` を保存できるようにする
6. `GeneratedAnswer.metadata` と `answers.jsonl.answer_metadata` は answerer 固有 metadata のみにする

この段階では再現性を優先し、設定は strict に扱います。

- unknown field は parse error にする
- 選択されていない kind の詳細設定 section は validate error にする
- backend / answerer は `[backend.<kind>]` / `[answerer.<kind>]` 形式で kind ごとの詳細設定を持てるようにする

この段階で LoCoMo / LongMemEval ごとに固定の設定ファイルを指定するだけで評価を実行できる状態にします。  
設定値の部分上書きは CLI では行わず、設定の単一の truth source は TOML に置きます。

### Phase 2: LLM Answerer を導入する （完了）

次に、回答生成を fixed 応答から実 LLM 呼び出しへ進めます。

1. `LlmAnswerer` を定義する
2. 暫定の共通 prompt builder を追加する
3. `LlmBackedAnswerer` を実装する
4. `rig-core` を使った OpenAI 互換 API 実装を追加する
5. TOML 設定ファイルから model / base URL / API key env / timeout / max retry / retry backoff などを読めるようにする
6. `api_key_env = None` のときは Authorization ヘッダなしで呼べるようにする
7. 解決済みの OpenAI 互換設定と retry 設定を `run.resolved.json` に残せるようにする
8. `DebugAnswerer` と LLM 実装を差し替えて動作確認する

この段階では judge は Phase 1 の暫定実装を維持し、ここで得られる answer correctness は **配線確認用の暫定値** と位置付けます。  
また、prompt builder も benchmark 固有仕様への完全準拠はまだ行わず、LoCoMo / LongMemEval の dataset-specific な回答 template と context profile は次の Phase で整理します。  
LoCoMo の F1 ベース評価や LongMemEval の official `evaluate_qa.py` / type-specific rubric への準拠は、その次の judge Phase で扱います。

### Phase 3: benchmark-specific Answer Prompt を入れる （完了）

次に、回答 prompt の構築を `Answerer` から切り離し、dataset / benchmark ごとの template 選択を明示します。

1. `PromptBuilder` を `Answerer` と独立したコンポーネントとして定義する
2. LoCoMo の category 1-4 / 5 で official answer template を切り替える
3. LongMemEval の `no-retrieval` / `history chats` / `history chats + facts` / `facts only` / `cot` / `Current Date` に対応する
4. backend が返す retrieval 結果とは別に `PromptContext` を扱えるようにする
5. `prompt_template_id` / `prompt_profile` を answer log に残せるようにする
6. `DebugAnswerer` と `LlmBackedAnswerer` の両方が同じ prompt 構築結果を受け取れるようにする

この Phase は完了済みです。  
LoCoMo / LongMemEval の公式 prompt 互換性そのものは今後の主目標ではありませんが、prompt 構築を独立コンポーネントとして切り出せたことで、以降の benchmark-specific 仕様変更を局所化できます。

### Phase 4: `locomo_kioku_v1` を実装する （完了）

次は LoCoMo の KIOKU 用評価仕様 `locomo_kioku_v1` を実装します。  
ここでは judge と retrieval を別 Phase に分けず、**1 つの評価フローとしてまとめて実装する** 方が進めやすいです。

理由は次です。

1. `locomo_kioku_v1` の retrieval 評価対象は `prompt_context.text` であり、backend の生 retrieval item ではない
2. retrieval judge と answerer は同じ `prompt_context.text` を共有する
3. retrieval log / answer log / aggregate metrics を同じ question 実行単位で更新する必要がある
4. 分割すると runner, logging, metrics の配線を 2 回触ることになり、手戻りが増える

したがって、この Phase では answer correctness と retrieval sufficiency を同時に benchmark semantics へ揃えます。

1. LoCoMo の実行パスを `locomo_kioku_v1` 前提に切り替える
2. category 1-4 のみを評価対象にする
3. `PromptContext` 必須の実行条件を runner / backend 契約に反映する
4. retrieval sufficiency judge を実装する
5. answer correctness judge を実装する
6. LoCoMo 用 answer prompt を `locomo.kioku.answer.v1` に揃える
7. `answers.jsonl`, `retrieval.jsonl`, `metrics.json`, `run.resolved.json` の semantics を `locomo_kioku_v1` に合わせる
8. `overall_answer_accuracy` と `overall_retrieval_sufficiency_accuracy`、および per-category 集計を出す
9. `judge_kind` / `metric_semantics_version` を `locomo_kioku_v1` に対応した値へ更新する

### Phase 5: LongMemEval の KIOKU 用評価仕様を実装する

LoCoMo が終わったら、次は LongMemEval の KIOKU 用評価仕様を実装します。  
ただし現時点では仕様が未確定なので、この Phase は **仕様確定が前提条件** です。

実装内容は仕様決定後に確定しますが、少なくとも次をこの Phase にまとめます。

1. LongMemEval の answer correctness semantics を実装する
2. LongMemEval の retrieval semantics を実装する
3. question type / abstention / context profile の扱いを仕様に合わせて整理する
4. answer logs / retrieval logs / aggregate metrics の semantics を LongMemEval 用に確定する
5. `judge_kind` / `metric_semantics_version` を LongMemEval 用仕様へ更新する

LongMemEval も LoCoMo と同様に、judge と retrieval を別々に段階化するより、仕様が固まった時点で一気に実装した方が runner と metrics の整合を取りやすいです。

### Phase 5.5: 旧 LongMemEval 評価仕様実装の完全削除

目的は、Phase 5 で `longmemeval_kioku_v1` への移行が完了したあとも、`crates/evaluate` に残っている **旧 LongMemEval 評価仕様の実装を完全に削除すること** です。

### Phase 5.6: retrieval contract を `prompt_context` 中心へ整理する

目的は、`crates/evaluate` に残っている **raw retrieval item 配列前提の I/F と artifact schema を削除し、LoCoMo / LongMemEval の KIOKU 評価 path を `prompt_context` 中心へ完全に寄せること** です。

この Phase での変更点は次です。

1. `QueryOutput` の正規出力を `prompt_context` と `metadata` に絞る
2. `RetrievedMemory` と `QueryOutput.retrieved` を評価の共通 contract から外す
3. `PromptBuildRequest.retrieved` を削除し、prompt builder は `prompt_context` のみを前提にする
4. `RetrievalLogRecord.retrieved_count` / `retrieved_memory_ids` / `retrieved_source_event_ids` を削除する
5. `DatasetMetrics.average_retrieved_item_count` を削除する
6. raw retrieval diagnostics が必要な場合は、共通 schema ではなく backend-specific metadata に閉じ込める

この変更により、LoCoMo / LongMemEval の評価意味論は次で統一されます。

- backend は evaluation-ready な `prompt_context` を返す
- runner は `prompt_context.text` を retrieval judge と answerer に共有する
- artifact は raw retrieval item 数ではなく、実際に answerer に渡した context と judge 結果を正規記録する

ここで重要なのは、`prompt_context` が retrieval の評価対象であり、raw retrieval item 配列はもはや共通評価 protocol の一部ではないことです。  
将来 backend 固有の診断情報が必要になった場合は、`metadata` に追加する方針を取ります。

### Phase 5.7: dataset-specific pipeline の共通化

目的は、`crates/evaluate` にある LoCoMo / LongMemEval 向けの 2 本の dataset-specific pipeline を、
**1 本の共通 runner と dataset-specific protocol 定義へ整理し直すこと** です。

### Phase 5.8: 旧 dataset-specific pipeline wrapper の削除

目的は、Phase 5.7 で互換性維持のために残した
`LoCoMoKiokuEvaluatePipeline` / `LongMemEvalKiokuEvaluatePipeline` の 2 つの wrapper を削除し、
**`CommonEvaluatePipeline` + `DatasetEvaluationProtocol` を実行系の正規入口として確定すること** です。

### Phase 6: 比較しやすい runner にする

最後に実験基盤としての使い勝手を上げます。

1. checkpoint / resume
2. 並列実行
3. token / latency / cost 収集
4. `run-name` や `output-dir` の追加
5. `full-context`, `oracle`, `return-all` の baseline 切り替え
6. `KiokuMemoryBackend` を追加して KIOKU 本体を接続する
7. location mapping と background-sync policy を `KiokuMemoryBackend` に実装する

## 13. この方針の利点

この構成にしておくと、KIOKU 本体の設計と評価基盤の設計を無理に共通化せずに済みます。

- `crates/core` は評価都合ではなく KIOKU のドメインに集中できる
- `crates/adapters/*` はインフラ差し替えの責務に集中できる
- dataset adapter は再利用できる
- Answerer は固定できる
- Judge は固定できる
- retrieval と answer を分離して比較できる
- 評価用のスタブ実装と本物の `KiokuMemoryBackend` を同じ runner で差し替えられる
- dataset 固有の location 表現と KIOKU 固有の `space / room / thread` を分離できる
- バックグラウンド処理シミュレーションの都合を runner に漏らさずに済む

つまり、ユーザが最初に考えていた

> 記憶層の実装がどうであっても、呼び出し I/F が同じなら評価できる仕組み

を、**評価プログラム側の I/F** としてそのまま実現できます。
