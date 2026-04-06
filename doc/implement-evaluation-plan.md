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

最初の完成条件は次です。

1. LoCoMo と LongMemEval を読み込める
2. 共通 runner で両方回せる
3. スタブ記憶層で動く
4. LoCoMo / LongMemEval それぞれに合った Judge で採点できる

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
スタブ実装では `ingest` を単純に push して即 return すれば十分です。

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

pub struct RetrievedMemory {
    pub memory_id: String,
    pub source_event_id: Option<String>,
    pub source_session_id: Option<String>,
    pub score: Option<f32>,
    pub timestamp: Option<Timestamp>,
    pub content: String,
    pub metadata: serde_json::Value,
}

pub struct QueryOutput {
    pub memories: Vec<RetrievedMemory>,
    pub metadata: serde_json::Value,
}
```

`QueryInput.timestamp` は質問時点の参照時刻を表すためのものであり、ingest 済み event の cutoff 時刻としては扱いません。
`QueryInput.budget` は answerer に渡す retrieval budget を表すためのものであり、backend 固有の検索パラメータを直接露出させません。
Phase 1 の `ReturnAllMemoryBackend` では `max_items` のみを実装し、`max_tokens` は将来の backend 向け予約フィールドとします。Phase 1.5 で導入する TOML 設定ファイルでは `max_tokens` を設定項目として保持できますが、Phase 1 系 backend では未対応のため明示的にエラーを返します。

retrieval 指標のために、`source_event_id` と `source_session_id` は必ず持たせます。

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

## 7. Answerer と Judge

### 7.1 Answerer

Answerer は記憶層共通で固定します。

```rust
#[async_trait]
pub trait Answerer {
    async fn answer(
        &self,
        dataset: DatasetKind,
        question: &BenchmarkQuestion,
        retrieved: &[RetrievedMemory],
    ) -> anyhow::Result<GeneratedAnswer>;
}
```

最初に必要なのは 2 種類だけです。

- `LlmAnswerer`: retrieved memories を prompt に詰めて回答させる
- `DebugAnswerer`: retrieved memories をそのまま出すか、固定応答を返す

比較実験では、**Answerer の prompt / model はバックエンド間で固定** します。

### 7.2 Judge

Judge は dataset ごとに分けます。

```rust
#[async_trait]
pub trait Judge {
    async fn judge(
        &self,
        question: &BenchmarkQuestion,
        generated: &GeneratedAnswer,
    ) -> anyhow::Result<Judgement>;
}
```

### LoCoMoJudge

- 主指標: LLM Judge
- 補助指標: Exact Match / F1 / BLEU-1
- category 1-4 を main score
- category 5 は別枠

### LongMemEvalJudge

- official `evaluate_qa.py` と同じ type-specific prompt を使う
- `task-averaged accuracy`, `overall accuracy`, `abstention accuracy` を出す

LongMemEval は generic judge で簡略化しない方がよいです。

## 8. runner の流れ

runner は dataset に依らず次の順で動かします。

1. raw dataset を load
2. dataset adapter で `BenchmarkCase` に変換
3. case ごとに backend を `reset`
4. `events` を順に `ingest`
5. question 時点で `query`
6. retrieval 結果を保存
7. `Answerer` で最終回答を生成
8. `Judge` で採点
9. metrics を集計して report を出す

この流れは `EverOS/evaluation` の 4-stage pipeline に近く、理解しやすいです。

## 9. スタブ記憶層の実装

最初に作るべきバックエンドは `ReturnAllMemoryBackend` です。

仕様は単純で十分です。

- `reset`: 内部の `Vec<StoredEvent>` を空にする
- `ingest`: 受け取った `MemoryEvent` を push
- `query`: その case に追加済みの全 event を timestamp 順で返す

追加で次の性質を持たせると便利です。

- `source_event_id` を必ず返す
- `source_session_id` を必ず返す
- `score` は `None` か固定値
- `budget.max_items` が指定されたら、時系列で新しい N 件だけを選び、返却順は時系列昇順に保つ

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
- `retrieved_memory_ids`
- `retrieved_event_ids`
- `retrieved_session_ids`
- `retrieval_budget`
- `latency_ms`
- `retrieval_metrics`
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

Phase 2 で `openai-compatible` answerer を導入したら、`run.resolved.json` と answerer 固有 metadata にはさらに次を追加します。

- `model`
- `base_url`
- `temperature`
- `max_output_tokens`
- `timeout_secs`
- `api_key_env`

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
├── answerer/
│   ├── mod.rs
│   ├── debug.rs
│   ├── llm.rs
│   ├── prompt.rs
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

Phase 1 が大きすぎると、runner の配線確認と LLM 実装・評価品質改善が混ざって進捗判定が曖昧になります。  
そのため、最初の完了条件は **「stub で end-to-end を通すこと」** に限定し、その後に設定基盤を整理してから LLM 統合へ進めます。

### Phase 1: stub で runner を完走させる

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

### Phase 1.5: TOML 設定ファイルへ移行する

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

### Phase 2: LLM Answerer を導入する

次に、回答生成を fixed 応答から実 LLM 呼び出しへ進めます。

1. `LlmAnswerer` を定義する
2. prompt builder を追加する
3. `LlmBackedAnswerer` を実装する
4. `rig-core` を使った OpenAI 互換 API 実装を追加する
5. TOML 設定ファイルから model / base URL / API key env / timeout などを読めるようにする
6. `api_key_env = None` のときは Authorization ヘッダなしで呼べるようにする
7. `DebugAnswerer` と LLM 実装を差し替えて動作確認する

### Phase 3: retrieval 指標を入れる

次に retrieval 側を評価可能にします。

1. LoCoMo の `evidence` に対する `hit_any@k`, `recall_all@k`, `mrr`
2. LongMemEval の session-level 指標
3. LongMemEval の turn-level 指標
4. abstention を retrieval 集計から除外

### Phase 4: 比較しやすい runner にする

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
