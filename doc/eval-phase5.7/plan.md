# Phase 5.7 実施計画

## 1. 目的

Phase 5.7 の目的は、`crates/evaluate` にある LoCoMo / LongMemEval 向けの 2 本の dataset-specific pipeline を、
**1 本の共通 runner と dataset-specific protocol 定義へ整理し直すこと** です。

ここで狙うのは、LoCoMo と LongMemEval の評価仕様を無理に同一化することではありません。  
両者の差分は維持したまま、次の共通フローだけを runner として共有します。

1. case scope を reset する
2. events を ingest する
3. backend に query する
4. retrieval judge を実行する
5. answer prompt を組み立てる
6. answerer を実行する
7. answer judge を実行する
8. logs を構築する
9. metrics 入力へ正規化する
10. dataset-specific metrics を集計する

現在の LoCoMo / LongMemEval は実行順序も artifact の作り方もかなり近く、  
差分の本体は主に次の 3 点です。

- 対象質問をどう絞るか
- answer prompt / judge prompt をどう構成するか
- metrics をどう集計するか

したがって Phase 5.7 では、`pipeline` を 2 本持つ設計から、
**共通 runner 1 本 + protocol 差し替え** へ移行する計画を固めます。

## 2. 前提整理

### 2.1 現状コードの観察

現在の主要な実行パスは次です。

1. `crates/evaluate/src/runner/locomo_kioku.rs`
   - LoCoMo 用 pipeline
   - category `1..=4` のみを評価対象にする
   - metrics は category 単位で集計する
2. `crates/evaluate/src/runner/longmemeval_kioku.rs`
   - LongMemEval 用 pipeline
   - 全 questions を評価対象にする
   - `question_type` / `is_abstention` を metrics に使う
   - context token count を使う
3. `crates/evaluate/src/runner/metrics.rs`
   - LoCoMo / LongMemEval の集計関数が別々に存在する
4. `crates/evaluate/src/prompt/answer.rs`
   - `PromptBuildRequest` は `AnswerPromptProfile` enum で single-source 化済み
5. `crates/evaluate/src/runner/policy.rs`
   - `ContextTokenPolicy::{Optional, Required}` が導入済み
6. `crates/evaluate/src/cli/evaluate.rs`
   - dataset ごとに pipeline 構築関数が分かれている

### 2.2 すでに整理できていること

Phase 5.7 を始める時点で、共通化に向けた下地は次のように揃っています。

- `BenchmarkCase` / `BenchmarkQuestion` / `QueryInput` は dataset 共通 model である
- `AnswerJudge` / `RetrievalJudge` は dataset 共通 trait である
- `PromptBuilder` は dataset-specific profile を引数に取る単一 trait である
- `PromptBuildRequest` は `dataset + 2 つの optional config` ではなく、`AnswerPromptProfile` で型安全になっている
- `ContextTokenPolicy` により、「必ず数える」と「数えなくてよい」を runner 境界で表現できる

つまり、runner 共通化を妨げているのは prompt 型安全性や token count の有無ではなく、
**dataset 固有ルールを今は pipeline 本体に埋め込んでいること** です。

### 2.3 現状の問題

現状構成には次の問題があります。

1. LoCoMo / LongMemEval で実行ループが重複している
2. bug fix や log schema 修正が両 pipeline に波及しやすい
3. 新しい dataset を追加すると、3 本目の pipeline を増やしやすい構造になっている
4. `cli/evaluate.rs` が dataset ごとの pipeline 組み立て責務を持ちすぎている
5. 実行ループと dataset 固有ルールの境界が曖昧で、どこを共通化すべきか読み取りにくい

## 3. Phase 5.7 の方針

### 3.1 共通化対象は「runner の骨格」であり、metrics semantics ではない

Phase 5.7 では、LoCoMo と LongMemEval の metrics semantics を 1 つにまとめません。  
共通化するのはあくまで runner の骨格です。

残す差分:

- 質問フィルタ
- prompt profile の選択
- context token count の要否
- metrics input への正規化方法
- metrics builder
- resolved metadata / provenance の一部

共通化する部分:

- case loop
- event ingest
- query 実行
- retrieval judge 実行
- answer prompt build
- answerer 実行
- answer judge 実行
- answer / retrieval log の基本構築

### 3.2 dataset-specific 差分は protocol trait へ押し出す

共通 runner を成立させるため、LoCoMo / LongMemEval の差分は
`DatasetEvaluationProtocol` のような trait に閉じ込めます。

責務イメージ:

```rust
trait DatasetEvaluationProtocol {
    type MetricInput;

    fn dataset(&self) -> BenchmarkDataset;
    fn context_token_policy(&self) -> ContextTokenPolicy;
    fn include_question(&self, question: &BenchmarkQuestion) -> bool;
    fn answer_prompt_profile<'a>(&'a self) -> AnswerPromptProfile<'a>;
    fn build_metric_input(
        &self,
        evaluated: &EvaluatedQuestion<'_>,
    ) -> anyhow::Result<Self::MetricInput>;
    fn build_metrics(
        &self,
        inputs: &[Self::MetricInput],
        context_tokenizer: Option<&str>,
    ) -> MetricsReport;
}
```

この trait 名や引数の形は実装時に微調整してよいですが、
重要なのは次の設計原則です。

1. protocol は dataset 固有の「方針」を持つ
2. runner は protocol の手順に従って実行するだけにする
3. runner は LoCoMo / LongMemEval の個別知識を持たない
4. runner は `cases.dataset` と `protocol.dataset()` の不一致を fail-fast する

### 3.3 context token count は protocol の policy で制御する

Phase 5.7 では LoCoMo に無理に token count を入れません。  
代わりに protocol が `ContextTokenPolicy` を返し、runner はそれに従います。

方針:

- `Required`
  - `TokenCounter` が必要
  - `context_token_count` を計算し、metrics input に渡す
  - provenance に tokenizer 名を渡せる
- `Optional`
  - runner は token count を計算しなくてよい
  - protocol 側が不要として扱う

この方式により、
「共通 runner は token counting の仕組みを知っているが、必ずしも毎回使わない」
という状態を自然に表現できます。

### 3.4 prompt profile の選択は protocol が担当する

`PromptBuildRequest` はすでに `AnswerPromptProfile` enum へ整理済みです。  
Phase 5.7 では runner が `locomo_kioku` / `longmemeval_kioku` を直接触らず、
protocol から `AnswerPromptProfile` を受け取るように寄せます。

これにより runner は prompt config の具体型を知らずに済みます。

### 3.5 token counter の注入方式は「常に optional で持つ」側へ寄せる

現在は LongMemEval pipeline の struct だけが `token_counter` を持っています。  
共通 runner 化では、runner 自身が `Option<&dyn TokenCounter>` を持つ構成へ寄せるのが自然です。

方針:

- protocol が `Required` を返すのに `token_counter` が `None` なら fail-fast
- protocol が `Optional` を返すなら `token_counter` は `None` でもよい

この形にすると、CLI 側の分岐も整理しやすくなります。

### 3.6 protocol は dataset-specific config だけを保持する

共通 runner 化の目的は、runner 本体を dataset 固有 config schema から切り離すことです。  
そのため protocol は `PromptConfig` 全体ではなく、
自分に必要な dataset-specific prompt config / metrics provenance 入力だけを保持する方針にします。

方針:

- protocol は LoCoMo / LongMemEval それぞれに必要な prompt config を個別に受け取る
- runner は `PromptConfig` 全体や config enum の分岐を知らない
- config schema の解決は引き続き CLI / config 層で行う

## 4. 完了条件

Phase 5.7 の完了条件は次です。

1. LoCoMo / LongMemEval 共通の runner 本体が `crates/evaluate/src/runner/` に追加されている
2. LoCoMo / LongMemEval の実行ループ重複が解消されている
3. dataset 固有差分が protocol trait へ分離されている
4. LoCoMo は `ContextTokenPolicy::Optional` を返し、LongMemEval は `Required` を返す
5. prompt profile の選択が pipeline 本体ではなく protocol 側の責務になっている
6. LoCoMo / LongMemEval の answer / retrieval log schema は変更しない
7. LoCoMo / LongMemEval の metrics semantics は変更しない
8. `cli/evaluate.rs` の dataset 分岐が「protocol と judge 構築」の責務に縮小されている
9. 既存の LoCoMo / LongMemEval テストが共通 runner 経由でも通る
10. 将来 3 本目の dataset を追加する際に、新 pipeline を複製せず、protocol / prompt / judge などの dataset-specific 実装追加で対応できる構造になっている

## 5. 対象設計

### 5.1 追加する主要コンポーネント

追加候補:

- `crates/evaluate/src/runner/pipeline.rs`
  - 共通 runner 本体
- `crates/evaluate/src/runner/protocol.rs`
  - dataset-specific protocol trait
- `crates/evaluate/src/runner/protocol/locomo.rs`
  - LoCoMo protocol 実装
- `crates/evaluate/src/runner/protocol/longmemeval.rs`
  - LongMemEval protocol 実装

最終的な file 構成は実装時に多少変えてよいですが、
`common runner` と `dataset-specific protocol` を module 境界で分ける方針は維持します。

### 5.2 共通 runner が持つべき責務

共通 runner の責務は次です。

1. dataset の一致確認
2. `cases.dataset` と `protocol.dataset()` の整合性確認
3. case ごとの backend reset
4. event ingest
5. question 反復
6. protocol による question 採用判定
7. backend query
8. retrieval judge 実行
9. answer prompt の構築
10. answerer 実行
11. answer judge 実行
12. `EvaluatedQuestion` の構築
13. answer / retrieval log の共通部分構築
14. protocol 用 metrics input の収集
15. protocol へ metrics build を委譲

runner は次を持たないようにします。

- LoCoMo の category `1..=4` という知識
- LongMemEval の abstention 集計という知識
- LoCoMo / LongMemEval の prompt config 具体型知識
- LoCoMo / LongMemEval の metrics builder 名

### 5.3 protocol が持つべき責務

protocol の責務は次です。

1. dataset kind の宣言
2. 質問フィルタ
3. context token policy
4. prompt profile の選択
5. `EvaluatedQuestion` から metrics input への変換
6. dataset-specific metrics build
7. dataset-specific prompt config / metrics provenance 入力の保持
8. 必要なら protocol 固有の validation

この分離により、
「LoCoMo と LongMemEval は同じ runner を使うが、何を評価してどう集計するかは別」
という構造を明確にできます。

## 6. 実装手順

### Step 1. 共通 runner の入出力モデルを定義する

まず `common runner`（共通 pipeline） と `protocol trait` の境界を決めます。

実施内容:

1. `runner/pipeline.rs` を追加する
2. `runner/protocol.rs` を追加する
3. `EvaluatedQuestion` を定義し、runner と protocol の共通受け渡し型にする
4. protocol が返す `MetricInput` と `EvaluatedQuestion` の関係を整理する
5. `ContextTokenPolicy` を protocol 経由で参照する形を決める
6. `dataset()` による protocol/case 整合性チェックを common runner の責務として固定する

この Step の目的は、
LoCoMo / LongMemEval の現在の差分を吸収できる最小 trait 境界を決めることです。

### Step 2. LoCoMo / LongMemEval の question filtering を protocol へ移す

現状の分岐のうち最も単純な差分から外へ出します。

実施内容:

1. LoCoMo の `category 1..=4` 判定を protocol 実装へ移す
2. LongMemEval の全 questions 採用を protocol 実装へ移す
3. runner 本体から dataset 固有の `filter()` を消す

この Step の目的は、
共通 loop が dataset-specific filtering を知らない状態にすることです。

### Step 3. prompt profile の選択を protocol へ移す

次に answer prompt 構築の dataset 差分を外へ出します。

実施内容:

1. protocol から `AnswerPromptProfile` を返せるようにする
2. LoCoMo protocol は `AnswerPromptProfile::LoCoMoKioku(...)` を返す
3. LongMemEval protocol は `AnswerPromptProfile::LongMemEvalKioku(...)` を返す
4. runner は `PromptBuildRequest` を protocol 由来 profile だけで組み立てる

この Step の目的は、
runner が prompt config の具体型を知らない状態にすることです。

### Step 4. context token counting を common runner に寄せる

LongMemEval 専用だった token count 分岐を common runner へ移します。

実施内容:

1. common runner は `Option<&dyn TokenCounter>` を受け取る
2. protocol の `ContextTokenPolicy` に従って token count を実施する
3. `Required` かつ `token_counter == None` の場合は error にする
4. `Optional` の場合は count しない
5. protocol の metrics input へ `Option<usize>` で渡すか、`Required` 時だけ `usize` を要求する形を決める

この Step の目的は、
token count を dataset-specific pipeline ではなく runner の汎用機能へ移すことです。

### Step 5. answer / retrieval log 構築を common runner に寄せる

ログ構築は現在ほぼ重複しているため、共通化効果が大きい箇所です。

実施内容:

1. `RetrievalLogRecord` の共通部分を common runner で組み立てる
2. `AnswerLogRecord` の共通部分を common runner で組み立てる
3. `context_kind_name` / `merge_metadata` / `sanitize_answer_metadata` は common runner から使う

この Step の目的は、
artifact schema 修正時の 2 箇所修正をやめることです。

### Step 6. metrics input 構築だけ protocol に残す

集計ルールは dataset-specific なので、ここは protocol 側へ残します。

実施内容:

1. common runner は 1 回の評価結果を `EvaluatedQuestion` として protocol へ渡す
2. protocol は `EvaluatedQuestion` を `MetricInput` に変換する
3. LoCoMo は category ベース input を返す
4. LongMemEval は question type / abstention / token count ベース input を返す

この Step の目的は、
共通 loop と dataset 固有 metrics semantics を無理に混ぜないことです。

### Step 7. LoCoMo / LongMemEval 専用 pipeline を薄い adapter に縮退させる

移行途中では既存 public struct を完全削除しない方が安全です。

実施内容:

1. `LoCoMoKiokuEvaluatePipeline` は common runner を呼ぶ薄い wrapper にする
2. `LongMemEvalKiokuEvaluatePipeline` も同様にする
3. 既存テストが大きく壊れないよう public API の変更を最小化する

この Step の目的は、
refactor のリスクを抑えつつ内部実装だけ共通化することです。

### Step 8. CLI の責務を整理する

最後に CLI の dataset 分岐を縮小します。

実施内容:

1. CLI は dataset ごとの judge 構築と protocol / wrapper 選択を行う
2. token counter の有無は protocol policy に合わせて渡す
3. dataset を `match` した後は generic な common runner / wrapper 呼び出しへ寄せる
4. `run_locomo_kioku` / `run_longmemeval_kioku` の重複を削減する

この Step の目的は、
runner 共通化の効果を CLI まで反映することです。

## 7. 影響範囲

### 7.1 runner

対象:

- `crates/evaluate/src/runner/mod.rs`
- `crates/evaluate/src/runner/locomo_kioku.rs`
- `crates/evaluate/src/runner/longmemeval_kioku.rs`
- 新規 `crates/evaluate/src/runner/pipeline.rs`
- 新規 protocol module

やること:

1. 共通 loop の抽出
2. protocol trait の導入
3. wrapper 化

### 7.2 prompt

対象:

- `crates/evaluate/src/prompt/answer.rs`

やること:

1. `AnswerPromptProfile` を common runner / protocol から使う
2. runner が prompt config の具体型に依存しないことを確認する
3. protocol が `PromptConfig` 全体ではなく dataset-specific prompt config を保持する構成へ寄せる

### 7.3 metrics

対象:

- `crates/evaluate/src/runner/metrics.rs`

やること:

1. 既存の LoCoMo / LongMemEval metrics builder は維持する
2. protocol から呼ぶ構成へ寄せる
3. metrics semantics は変えない

### 7.4 CLI

対象:

- `crates/evaluate/src/cli/evaluate.rs`

やること:

1. dataset-specific pipeline 構築重複を減らす
2. token counter の注入を common runner 前提へ整理する
3. dataset `match` 後の呼び出しは generic な common runner / wrapper へ寄せる

## 8. リスクと対策

### 8.1 trait が大きくなりすぎる

protocol trait に多くの責務を載せすぎると、逆に理解しにくくなります。

対策:

- 共通 runner が必要とする最小責務だけを持たせる
- prompt / metrics / filtering の 3 系統に絞る
- backend / judge の生成責務は protocol に入れない

### 8.2 LoCoMo / LongMemEval の semantics を誤って変えてしまう

共通化の過程で metrics 分母や filter 条件が変わる危険があります。

対策:

- 既存 test を維持し、wrapper 経由でも同じ期待値を確認する
- `metrics.json` の schema と値を regression test で固定する
- LoCoMo category filter と LongMemEval abstention 集計は protocol test で明示する

### 8.3 token counter の optional 化で LongMemEval の fail-fast が弱くなる

対策:

- `ContextTokenPolicy::Required` なら runner が必ず fail-fast する
- LongMemEval 用 test で `token_counter` 欠落時 error を追加する

### 8.4 CLI が中途半端に二重構造になる

対策:

- wrapper を残す場合でも、共通 runner 呼び出しは 1 箇所に寄せる
- dataset 分岐は judge / protocol 選択までにとどめる
- dataset を `match` した後は generic な共通関数を呼ぶ形へ寄せる

## 9. 検証計画

### 9.1 既存テストの維持

少なくとも次を維持します。

- LoCoMo pipeline の category 5 skip test
- LoCoMo の retrieval judge / answerer context 共有 test
- LongMemEval の abstention 分離 metrics test
- LongMemEval の retrieval judge / answerer context 共有 test
- output schema test

### 9.2 追加するべきテスト

Phase 5.7 では次の追加 test が必要です。

1. common runner が protocol の question filter に従う test
2. common runner が `ContextTokenPolicy::Required` で token counter を要求する test
3. common runner が `ContextTokenPolicy::Optional` で token counter なしでも動く test
4. common runner が `cases.dataset` と `protocol.dataset()` の不一致で fail-fast する test
5. LoCoMo wrapper が common runner を経由しても既存 metrics を返す test
6. LongMemEval wrapper が common runner を経由しても既存 metrics を返す test

### 9.3 目視確認

少なくとも次を `rg` で確認します。

- 共通 loop の重複が減っていること
- `cli/evaluate.rs` の `run_locomo_kioku` / `run_longmemeval_kioku` の差分が縮小していること
- runner 本体に `category 1..=4` や `question_type` 固有知識が残っていないこと

## 10. 実施しないこと

Phase 5.7 では次はやりません。

1. LoCoMo に token count を導入すること
2. LoCoMo / LongMemEval の metrics schema を統一すること
3. judge 実装を 1 本化すること
4. config schema を全面的に組み替えること
5. 新 dataset の追加

Phase 5.7 はあくまで、
**既存の LoCoMo / LongMemEval semantics を維持したまま、runner の実装重複を解消する Phase**
として扱います。
