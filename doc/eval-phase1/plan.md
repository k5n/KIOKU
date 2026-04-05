# Phase 1 実装計画

## 1. 目的

Phase 1 の目的は、`crates/evaluate` に **LoCoMo / LongMemEval の両方を同じ runner で回せる最小の end-to-end 評価基盤** を実装することです。

この段階では、記憶層そのものの性能改善や LLM 連携はまだ行いません。まずは次を成立させます。

1. dataset を共通の `BenchmarkCase` に正規化できる
2. `MemoryBackend` を差し替え可能な runner がある
3. `Answerer` の trait を先に確定できる
4. fixed 文字列を返す `DebugAnswerer` で全体が最後まで動く
5. 生成回答ログと簡易 metrics を出力できる

Phase 1 は「最終的な評価品質」を完成させる段階ではなく、**評価実験の土台を壊れにくい形で先に作る段階** と位置付けます。

## 2. Phase 1 の完了条件

Phase 1 の完了条件は次です。

1. `crates/evaluate` に評価用の共通データ型が入っている
2. `MemoryBackend` trait と `ReturnAllMemoryBackend` が実装されている
3. `Answerer` trait が定義されている
4. `DebugAnswerer` が実装され、fixed 文字列を返せる
5. dataset adapter が LoCoMo / LongMemEval を `BenchmarkCase` に変換できる
6. 共通 runner が `reset -> ingest -> query -> answer -> judge` を回せる
7. Judge は Phase 1 用の最小実装で動作する
8. CLI から dataset と backend と answerer を指定して 1 回の評価を実行できる
9. `answers.jsonl`, `retrieval.jsonl`, `metrics.json` を出力できる
10. Phase 1 の metrics は比較用最終スコアではなく、runner / adapter / logging の配線確認用だと明記されている

## 3. 前提整理

### 3.1 Phase 1 のボトルネックは runner 全体の配線確認

現時点で先に片付けるべきなのは、LLM 呼び出し I/F の設計よりも、評価 runner 全体の配線確認です。

ただし、ここで重要なのは次の点です。

- runner がまず必要とするのは「質問と retrieved memory を受けて最終回答を返すもの」であって、HTTP 実装や OpenAI 固有 API ではない
- したがって Phase 1 では、まず **`Answerer` の抽象 I/F** だけを決める
- 実装は先に `DebugAnswerer` を入れ、LLM 連携は Phase 2 へ送る

この順にすることで、外部 API 呼び出しにも LLM 抽象の詳細設計にも依存せず runner 全体を先に完成させられます。

### 3.2 Phase 1 では Judge は最小実装でよい

`doc/evaluation.md` では最終的に dataset ごとの適切な judge を使う方針ですが、Phase 1 では「全体を回す」ことが主目的です。

したがってこの段階では次を採ります。

- LoCoMo: まずは exact match ベースの簡易 judge
- LongMemEval: まずは exact match + abstention 判定の簡易 judge
- type-specific prompt や LLM judge は Phase 3 以降に強化する

ここで judge を作り込み過ぎると、肝心の runner と I/F 固定が遅れます。

## 4. 実装方針

### 4.1 先に trait を固定する

Phase 1 では、まず以下の 2 つの trait を先に確定します。

1. `MemoryBackend`
2. `Answerer`

この 2 つが確定すれば、記憶検索、回答生成、採点を独立に差し替えられます。

### 4.2 Phase 1 は stub で end-to-end を通すことに集中する

Phase 1 では回答品質を追いません。

- `DebugAnswerer` で固定応答を返す
- `ReturnAllMemoryBackend` で ingest 済み event を返す
- judge は簡易版に留める

これにより、「runner が動くこと」と「dataset adapter が正しく配線されていること」を先に検証します。

## 5. 推奨モジュール構成

Phase 1 時点では次の構成を目標にします。

```text
crates/evaluate/src/
├── lib.rs
├── model/
│   ├── mod.rs
│   ├── benchmark.rs
│   ├── retrieval.rs
│   ├── answer.rs
│   └── metrics.rs
├── backend/
│   ├── mod.rs
│   ├── traits.rs
│   └── return_all.rs
├── datasets/
│   ├── mod.rs
│   ├── locomo.rs
│   └── longmemeval.rs
├── answerer/
│   ├── mod.rs
│   ├── traits.rs
│   └── debug.rs
├── judge/
│   ├── mod.rs
│   ├── traits.rs
│   ├── locomo.rs
│   └── longmemeval.rs
├── runner/
│   ├── mod.rs
│   └── pipeline.rs
├── cli/
│   ├── mod.rs
│   └── evaluate.rs
└── bin/
    └── evaluate.rs
```

既存の `src/bin/locomo.rs` と `src/bin/longmemeval.rs` にある型定義は、Phase 1 で `datasets/` 配下へ移し、bin は thin wrapper に寄せます。

## 6. 先に確定する I/F

### 6.1 `MemoryBackend`

`MemoryBackend` は既存方針どおり、評価 runner 専用 I/F とします。

```rust
pub struct RetrievalBudget {
    pub max_items: Option<usize>,
    pub max_tokens: Option<usize>,
}

#[async_trait]
pub trait MemoryBackend {
    async fn reset(&mut self, scope: EvalScope) -> anyhow::Result<()>;
    async fn ingest(&mut self, event: MemoryEvent) -> anyhow::Result<()>;
    async fn query(&mut self, input: QueryInput) -> anyhow::Result<QueryOutput>;
}
```

`QueryInput.timestamp` は質問時点の参照時刻を表すためのものであり、`ReturnAllMemoryBackend` では ingest 済み event の cutoff 時刻としては扱いません。
`QueryInput.budget` は answerer に渡す retrieval budget を表すためのものであり、Phase 1 の `ReturnAllMemoryBackend` では `max_items` だけを使って返却件数を制御します。`max_tokens` は将来の backend 向け予約フィールドとし、Phase 1 の CLI では指定された場合にエラーとします。

### 6.2 `Answerer`

`Answerer` は runner が直接使うインターフェースです。

```rust
#[async_trait]
pub trait Answerer {
    async fn answer(
        &self,
        request: AnswerRequest<'_>,
    ) -> anyhow::Result<GeneratedAnswer>;
}
```

`AnswerRequest` には次を含めます。

- `dataset`
- `case`
- `question`
- `retrieved`

この形にしておくと、将来 prompt を dataset ごとに調整したり、`case` の metadata を参照したりできます。

## 7. `Answerer` の実装方針

### 7.1 `DebugAnswerer`

最初に作る実装は `DebugAnswerer` です。

役割は次です。

- `Answerer` を直接実装し、LLM を実際には呼ばない
- fixed 文字列をそのまま返す
- runner 全体の配線確認に使う

Phase 1 では回答品質は求めません。既定値は次のような fixed 文字列で十分です。

```text
[debug-answer]
```

`GeneratedAnswer.metadata` には最低限次を残します。

- `answerer_kind: "debug"`
- `mode: "fixed"`
- `case_id`
- `question_id`
- `retrieved_count`

これにより、回答本文は固定でもログ上で runner の配線確認ができます。

## 8. Dataset adapter の実装計画

### 8.1 LoCoMo

既存の `src/bin/locomo.rs` の型を `datasets/locomo.rs` へ移します。

追加タスクは次です。

1. raw JSON を読む loader を作る
2. `ConversationEntry` から `BenchmarkCase` へ変換する adapter を作る
3. `question_id` を `locomo:{sample_id}:q{idx}` で決定的に生成する
4. `event_id` を `locomo:{sample_id}:event:{dia_id}` で決定的に生成する
5. `session_X_date_time` と `session_X` のキーをパースし、`session_X` が存在する session については開始日時も存在することを検証する
6. `session_X` を持たない orphan な `session_X_date_time` は、公式配布データ互換のため無視する
7. session を `session_X` の数値 ID で sort してから event を生成する
8. turn ごとの疑似 timestamp を生成する
9. loader / adapter test で、session 対応付けと timestamp の単調増加を固定する
10. `evidence` の `dia_id` を同じ規則で `evidence_event_ids` に正規化する
11. `category == 5` のとき `adversarial_answer` を優先できるよう保持する

### 8.2 LongMemEval

既存の `src/bin/longmemeval.rs` の型を `datasets/longmemeval.rs` へ移します。

追加タスクは次です。

1. `LongMemEvalMessage` に `has_answer: Option<bool>` を追加する
2. loader を作り、`haystack_dates` / `haystack_session_ids` / `haystack_sessions` の長さ一致を fail-fast で検証する
3. `LongMemEvalEntry` から `BenchmarkCase` へ変換する adapter を作る
4. `question_id` を raw の `question_id` を用いて `longmemeval:{question_id}` に正規化する
5. `event_id` を `longmemeval:{question_id}:{session_id}:t{turn_idx}` で決定的に生成する
6. session を日付順に sort する
7. `answer_session_ids` を `evidence_session_ids` に入れる
8. `has_answer == true` の turn を同じ規則で `evidence_event_ids` に落とす
9. `question_id.ends_with("_abs")` で abstention を判定する

### 8.3 ID 正規化規則

Phase 1 では、dataset adapter ごとの差で fixture / log / retrieval metrics の結合規則がぶれないよう、`case_id` / `question_id` / `event_id` を決定的に固定します。

- LoCoMo:
  - `case_id`: `locomo:{sample_id}`
  - `question_id`: `locomo:{sample_id}:q{idx}`
  - `event_id`: `locomo:{sample_id}:event:{dia_id}`
- LongMemEval:
  - `case_id`: `longmemeval:{raw_question_id}`
  - `question_id`: `longmemeval:{raw_question_id}`
  - `event_id`: `longmemeval:{raw_question_id}:{session_id}:t{turn_idx}`

ここでの `idx` と `turn_idx` は 0-based で生成し、adapter test で規則を固定します。`evidence_event_ids`、`source_event_id`、`answers.jsonl`、`retrieval.jsonl` はすべてこの canonical ID を使います。

## 9. Backend 実装計画

### 9.1 `ReturnAllMemoryBackend`

Phase 1 の backend はこれだけで十分です。

内部表現は次のような単純な構造でよいです。

```rust
pub struct ReturnAllMemoryBackend {
    events: Vec<StoredEvent>,
}
```

`StoredEvent` に保持するもの:

- `event_id`
- `timestamp`
- `stream_id`
- `content`
- `speaker_id`
- `speaker_name`
- `metadata`

`query` は、取り込み済み event を timestamp 順に `RetrievedMemory` に変換して返します。`budget.max_items` が指定された場合は、時系列で新しい N 件だけを選び、返却順は時系列昇順に保ちます。`budget.max_tokens` は Phase 1 の `ReturnAllMemoryBackend` では未対応であり、CLI で指定された場合は明示的にエラーとします。

### 9.2 Phase 1 で backend に入れないもの

以下は Phase 1 では入れません。

- embedding
- rerank
- session 集約検索
- async background simulation
- KIOKU 本体との接続

ここを入れ始めると、Phase 1 の目的である「共通 runner の完成」から逸れます。

## 10. Judge 実装計画

### 10.1 共通 trait

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

### 10.2 Phase 1 の最小 judge

Phase 1 では次の簡易仕様にします。

- 文字列正規化:
  - trim
  - lowercase
  - 連続空白の圧縮
- LoCoMo:
  - normalized exact match
- LongMemEval:
  - normalized exact match
  - abstention 問題では、生成回答に「unknown」「not enough」「わからない」「情報が足りない」等が含まれるかも補助判定

`Judgement` は最低限次を持ちます。

- `is_correct: bool`
- `score: f32`
- `label: String`
- `metadata: serde_json::Value`

`Judgement.metadata` と `metrics.json` には、Phase 1 の簡易 judge と最終 benchmark judge を混同しないために、最低限次を残します。

- `judge_kind`
- `metric_semantics_version`
- `provisional: true`

LLM judge 化は Phase 3 以降で行います。

## 11. Runner 実装計画

### 11.1 1 case の処理順

runner の 1 case 処理は次の順に固定します。

1. backend `reset`
2. case の `events` を順に `ingest`
3. case の `questions` を順に処理
4. backend `query`
5. answerer `answer`
6. judge `judge`
7. log / metrics へ保存

### 11.2 最初に作る出力

Phase 1 の出力は絞ります。

- `answers.jsonl`
- `retrieval.jsonl`
- `metrics.json`

最低限これだけあれば、配線確認と簡易比較が可能です。

### 11.3 metrics

Phase 1 で出す集計は次です。

- overall accuracy (`LoCoMo` は `category 1-4` のみ)
- dataset ごとの question count
- LoCoMo per-category accuracy
- LongMemEval per-type accuracy
- abstention accuracy
- average retrieved item count

ただし、これらは最終比較用スコアではなく、配線確認用の簡易 metrics として扱います。

`metrics.json` には集計値に加えて最低限次の識別情報を入れます。

- `judge_kind`
- `metric_semantics_version`
- `provisional: true`
- `locomo_overall_scope: "category_1_4"`

## 12. CLI 実装計画

Phase 1 の CLI は単一エントリ `evaluate` に寄せます。

想定オプション:

- `--dataset locomo|longmemeval`
- `--input <path>`
- `--backend return-all`
- `--answerer debug`
- `--output-dir <path>`
- `--max-items <n>`
- `--max-tokens <n>`

`clap` を導入します。
ただし Phase 1 では `--max-tokens` は未対応オプションとして扱い、指定された場合は runner がエラーを返します。

## 13. 実装順序

1. `crates/evaluate` のモジュール再編
2. 既存 dataset 型を `datasets/` 配下へ移設
3. 共通 model 型を追加
4. `MemoryBackend` trait と `ReturnAllMemoryBackend` を実装
5. `Answerer` trait を定義
6. `DebugAnswerer` を実装
7. LoCoMo / LongMemEval adapter を実装
8. 最小 judge を実装
9. runner を実装
10. CLI を実装
11. `DebugAnswerer` で LoCoMo / LongMemEval を最後まで通す

## 14. テスト計画

Phase 1 で最低限入れるテストは次です。

1. LoCoMo loader のデシリアライズ test
2. LongMemEval loader のデシリアライズ test
3. LoCoMo loader が `session_X` に対する `session_X_date_time` の存在を検証し、orphan な `session_X_date_time` を無視した上で session ID 順に sort する test
4. LoCoMo adapter が `BenchmarkCase` を正しく作り、timestamp を単調増加で作る test
5. LongMemEval loader が parallel array の長さ不一致で fail-fast する test
6. LongMemEval adapter が abstention / `has_answer` を保持する test
7. LoCoMo / LongMemEval adapter の `question_id` / `event_id` 正規化規則を固定する test
8. `ReturnAllMemoryBackend` の `query` が時系列で新しい N 件を返す test
9. `--max-tokens` 指定時に CLI がエラーを返す test
10. `DebugAnswerer` の fixed answer test
11. runner の end-to-end test
12. judge の normalized exact match test

## 15. リスクと対策

### 15.1 trait を早く作り過ぎて硬直化するリスク

対策:

- `AnswerRequest` に `metadata` を持たせ、後から追加情報を逃がせるようにする
- I/F は最小限に留める

### 15.2 Judge 実装に引きずられて Phase 1 が肥大化するリスク

対策:

- Phase 1 の judge は簡易版に限定する
- official rubric 対応は Phase 3 へ送る

### 15.3 dataset adapter と runner の責務が混ざるリスク

対策:

- dataset adapter は `BenchmarkCase` 生成だけに責務を限定する
- timestamp 生成規則も adapter 側に閉じ込める

## 16. Phase 1 完了後に着手するもの

Phase 1 の次に進める対象は次です。

1. Phase 2 の `LlmAnswerer` / `LlmBackedAnswerer` / `rig-core` 統合
2. LoCoMo の retrieval metrics
3. LongMemEval の session-level / turn-level retrieval metrics
4. LoCoMo の LLM judge
5. LongMemEval の official type-specific judge
6. `full-context` / `oracle` backend
7. `KiokuMemoryBackend`

## 17. この計画で重要な設計判断

この Phase 1 で最も重要な判断は次の 3 点です。

1. `Answerer` だけを先に固定し、LLM 抽象は後続フェーズへ送る
2. runner が直接知るのは `Answerer` までで、LLM API 実装詳細は持ち込まない
3. judge は最初から作り込み過ぎず、まずは end-to-end を完成させる

この順序で進めると、外部 API や採点詳細に詰まらず、評価基盤そのものを先に安定化できます。
