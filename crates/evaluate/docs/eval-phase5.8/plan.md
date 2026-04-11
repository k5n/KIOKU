# Phase 5.8 実施計画

## 1. 目的

Phase 5.8 の目的は、Phase 5.7 で互換性維持のために残した
`LoCoMoKiokuEvaluatePipeline` / `LongMemEvalKiokuEvaluatePipeline` の 2 つの wrapper を削除し、
**`CommonEvaluatePipeline` + `DatasetEvaluationProtocol` を実行系の正規入口として確定すること** です。

ここで狙うのは、LoCoMo / LongMemEval の protocol をさらに共通化することではありません。  
共通 runner と protocol への分離はすでにできている前提で、
Phase 5.8 では「旧 dataset-specific pipeline 名を実行入口として残し続ける必要があるか」を整理し、
不要であれば撤去します。

言い換えると、Phase 5.7 が
**共通 runner の導入**
だったのに対し、
Phase 5.8 は
**移行のために残した暫定 wrapper の撤去**
です。

## 2. 前提整理

### 2.1 現状コードの観察

Phase 5.7 完了時点の実行構造は次です。

1. `crates/evaluate/src/runner/pipeline.rs`
   - `CommonEvaluatePipeline` が実行ループ本体を持つ
2. `crates/evaluate/src/runner/protocol.rs`
   - `DatasetEvaluationProtocol` と `EvaluatedQuestion` を定義する
3. `crates/evaluate/src/runner/protocol/locomo.rs`
   - LoCoMo 固有の filter / prompt profile / metrics 変換を持つ
4. `crates/evaluate/src/runner/protocol/longmemeval.rs`
   - LongMemEval 固有の filter / token policy / metrics 変換を持つ
5. `crates/evaluate/src/runner/locomo_kioku.rs`
   - `LoCoMoKiokuEvaluatePipeline` が protocol を組み立てて common runner を呼ぶ薄い wrapper
6. `crates/evaluate/src/runner/longmemeval_kioku.rs`
   - `LongMemEvalKiokuEvaluatePipeline` が protocol を組み立てて common runner を呼ぶ薄い wrapper
7. `crates/evaluate/src/cli/evaluate.rs`
   - judge 構築後に wrapper を経由して実行する

つまり現在は、
**共通 runner への移行は完了しているが、public / call-site 上は旧 pipeline 名がまだ残っている**
状態です。

### 2.2 wrapper が今やっていること

現在の 2 wrapper の責務は限定的です。

1. `PromptConfig` から dataset-specific prompt config を取り出す
2. 対応する protocol を生成する
3. `CommonEvaluatePipeline` に依存オブジェクトを渡して `run()` する

一方で、次はすでに wrapper の責務ではありません。

- question filter
- prompt profile の選択
- context token policy の判定
- answer / retrieval log 構築
- metrics input 変換
- metrics 集計

これらはすべて protocol または common runner に移っています。

### 2.3 現状の問題

wrapper を残したままだと、次のような問題が続きます。

1. 実行入口が `CommonEvaluatePipeline` と dataset-specific wrapper の二重構造に見える
2. `runner/mod.rs` の export 面に旧 pipeline 名が残り、設計の中心がどこか分かりにくい
3. CLI が protocol を直接扱わず、暫定 adapter に依存し続ける
4. 新 dataset 追加時に、本来必要な protocol / prompt profile・builder 対応 / judge / config 解決に加えて、「wrapper も足すべきか」という迷いが残る
5. Phase 5.7 で整理した責務境界が、型名の表面ではまだ読み取りにくい

## 3. Phase 5.8 の方針

### 3.1 正規の実行入口を common runner + protocol に一本化する

Phase 5.8 では、
`LoCoMoKiokuEvaluatePipeline` / `LongMemEvalKiokuEvaluatePipeline`
を正規 API として扱うのをやめます。

今後の基本構造:

1. CLI / 上位層が dataset-specific prompt config から protocol を組み立てる
2. 上位層が `CommonEvaluatePipeline` に protocol を渡す
3. 実行ループは common runner のみが持つ

この方針により、
「runner は 1 本、差分は protocol」
という Phase 5.7 の設計意図をコード表面でも一致させます。

### 3.2 dataset-specific の入口は protocol constructor に寄せる

wrapper を消す場合でも、dataset-specific の初期化責務は残ります。  
ただしそれは runner 型ではなく、protocol の生成側に寄せるべきです。

候補:

- `LoCoMoKiokuEvaluationProtocol::new(...)`
- `LongMemEvalKiokuEvaluationProtocol::new(...)`

必要なら将来的に factory 関数を追加してもよいですが、
少なくとも「薄い runner wrapper」を残す必要はありません。

### 3.3 `PromptConfig` 全体を runner に渡さない方向を徹底する

wrapper を残していると、`PromptConfig` 全体を持つ構造が温存されやすくなります。  
Phase 5.8 ではこれをさらに整理し、
common runner へ渡る時点では dataset-specific prompt config だけが解決済みである形を目指します。

方針:

- CLI / config 層で `PromptConfig` から必要な dataset-specific config を解決する
- protocol は自分に必要な config だけを保持する
- common runner は `PromptConfig` 全体を知らない

### 3.4 CLI の dataset 分岐は「judge と protocol の構築」に限定する

Phase 5.8 では `cli/evaluate.rs` の dataset 分岐を次まで縮めます。

1. dataset-specific judge を構築する
2. dataset-specific protocol を構築する
3. token counter の有無を決める
4. 共通の `CommonEvaluatePipeline` 実行へ渡す

CLI はここで
`LoCoMoKiokuEvaluatePipeline` という名前を知る必要がなくなります。

### 3.5 今回の対象は wrapper 削除であり、protocol の再設計ではない

Phase 5.8 では protocol trait の責務を大きく変えません。  
やるのは、Phase 5.7 で導入した設計をそのまま前面に出すことです。

今回はやらないこと:

- LoCoMo / LongMemEval protocol の統合
- metrics builder の統合
- prompt config schema の全面変更
- judge 構築の共通 factory 化

## 4. 完了条件

Phase 5.8 の完了条件は次です。

1. `crates/evaluate/src/runner/locomo_kioku.rs` が削除されている
2. `crates/evaluate/src/runner/longmemeval_kioku.rs` が削除されている
3. `runner/mod.rs` が wrapper 型を export しなくなっている
4. `cli/evaluate.rs` が protocol + common runner を直接使って実行している
5. `CommonEvaluatePipeline` が LoCoMo / LongMemEval の唯一の runner 本体として読める状態になっている
6. LoCoMo / LongMemEval の answer / retrieval log schema は変わらない
7. LoCoMo / LongMemEval の metrics semantics は変わらない
8. token counter の `Required` / `Optional` の振る舞いは変わらない
9. 既存テストが wrapper 削除後も通る
10. 新 dataset を追加する際の主要追加物が「protocol / prompt profile・builder 対応 / judge / config 解決」であることがコード表面でも明確になっている

## 5. 対象設計

### 5.1 変更対象

主対象:

- `crates/evaluate/src/runner/mod.rs`
- `crates/evaluate/src/runner/pipeline.rs`
- `crates/evaluate/src/runner/protocol.rs`
- `crates/evaluate/src/runner/protocol/locomo.rs`
- `crates/evaluate/src/runner/protocol/longmemeval.rs`
- `crates/evaluate/src/cli/evaluate.rs`

削除対象:

- `crates/evaluate/src/runner/locomo_kioku.rs`
- `crates/evaluate/src/runner/longmemeval_kioku.rs`

### 5.2 望ましい実行イメージ

最終形の責務イメージは次です。

```rust
let protocol = LoCoMoKiokuEvaluationProtocol::new(prompt);

let mut pipeline = CommonEvaluatePipeline {
    backend,
    prompt_builder,
    answerer,
    answer_judge,
    retrieval_judge,
    token_counter: None,
    budget,
    protocol: &protocol,
};

let result = pipeline.run(&cases).await?;
```

LongMemEval では `token_counter: Some(...)` を渡すだけで、
実行入口の形は同じにします。

### 5.3 境界の考え方

Phase 5.8 後の境界は次のように明確化します。

- common runner
  - 実行骨格を持つ唯一の runner
- protocol
  - dataset-specific policy を持つ
- prompt 層
  - dataset-specific prompt profile を answer prompt へ変換する
- CLI / config 層
  - config 解決と protocol / judge 構築を持つ

ここに dataset-specific runner wrapper は置きません。

## 6. 実装手順

### Step 1. CLI が直接 protocol を組み立てられるようにする

まず実行入口を wrapper から切り離します。

実施内容:

1. `cli/evaluate.rs` で dataset-specific prompt config を解決する
2. LoCoMo / LongMemEval protocol を CLI 側で直接生成する
3. `CommonEvaluatePipeline` を CLI 側で直接組み立てる
4. token counter の `Some / None` を CLI から直接渡す

この Step の目的は、
wrapper を消す前に call-site を common runner 直結へ寄せることです。

### Step 2. 共通実行 helper を generic helper に一本化する

現在 CLI には dataset ごとの薄い実行 helper と、
wrapper 前提の抽象が残っています。  
Phase 5.8 ではこれを残さず、
protocol を型引数に取る generic helper へ一本化します。

実施内容:

1. `run_locomo_kioku` / `run_longmemeval_kioku` を削除する
2. `run_pipeline_with_protocol` あるいは同等の thin composition helper に一本化する
3. `run_pipeline` / `EvaluateRunner` のような wrapper 前提抽象を削除する
4. helper が protocol と common runner だけを前提にすることを確認する

この Step の目的は、
dataset-specific runner 名を実行 helper からも消すことです。

### Step 3. wrapper module を削除する

実行入口の置き換えが済んだら、旧 wrapper を削除します。

実施内容:

1. `runner/locomo_kioku.rs` を削除する
2. `runner/longmemeval_kioku.rs` を削除する
3. `runner/mod.rs` から `mod` 宣言と `pub use` を削除する

この Step の目的は、
設計上だけでなく module 構造上も common runner 一本にすることです。

### Step 4. wrapper 依存のテストを移設する

既存の dataset-specific runner test は wrapper module 配下にあります。  
wrapper を削除すると、テストの置き場所を決め直す必要があります。

実施内容:

1. LoCoMo の category 5 skip test を protocol / pipeline 側へ移す
2. LoCoMo の retrieval judge / answerer context 共有 test を pipeline 側へ移す
3. LongMemEval の abstention metrics integration test を protocol / pipeline 側へ移しつつ、`runner/metrics.rs` の直接 test も維持する
4. LongMemEval の retrieval judge / answerer context 共有 test を pipeline 側へ移す
5. `context_token_policy()` の検証は protocol test に移すか、方針 test として再配置する

この Step の目的は、
wrapper 削除でテスト coverage が落ちないようにすることです。

### Step 5. export 面を整理する

最後に、外から見える API を新構造に合わせます。

実施内容:

1. `runner/mod.rs` の export を common runner / policy / result / output に整理する
2. protocol をどこまで外へ見せるかを決める
3. crate 内部だけでよいものは `pub(crate)` のままにする

この Step の目的は、
古い入口を参照しづらくし、新しい責務境界を型シグネチャで表すことです。

## 7. 影響範囲

### 7.1 runner

対象:

- `crates/evaluate/src/runner/mod.rs`
- `crates/evaluate/src/runner/pipeline.rs`
- `crates/evaluate/src/runner/protocol.rs`
- `crates/evaluate/src/runner/protocol/*`

やること:

1. wrapper 前提の export を削る
2. protocol / pipeline を唯一の実行面として整理する
3. dataset-specific test の再配置先を決める

### 7.2 CLI

対象:

- `crates/evaluate/src/cli/evaluate.rs`

やること:

1. protocol を直接構築する
2. common runner を直接実行する
3. wrapper 名への依存を消す
4. thin composition helper へ実行組み立てを集約する

### 7.3 tests

対象:

- 旧 wrapper module 配下の unit test
- common runner / protocol test

やること:

1. wrapper 削除後も LoCoMo / LongMemEval の主要回帰を維持する
2. test の責務を protocol / pipeline / output に再配置する
3. metrics builder の直接 test は維持し、integration test と役割分担させる

## 8. リスクと対策

### 8.1 API 変更で call-site が増える

wrapper を消すと、CLI や将来の呼び出し側が protocol を直接知る必要があります。

対策:

- protocol constructor を分かりやすく保つ
- generic helper を 1 つに揃える
- 「runner wrapper を戻す」のではなく「protocol 初期化 helper」を検討する

### 8.2 test の置き場所が曖昧になる

今は dataset-specific test が wrapper module にぶら下がっています。

対策:

- filter / token policy / metric input は protocol test に寄せる
- 実行ループの挙動は pipeline test に寄せる
- metrics semantics の直接検証は `runner/metrics.rs` の test を維持する
- output schema は引き続き output test で固定する

### 8.3 `PromptConfig` 全体依存が別の形で残る

wrapper を消しても、CLI が雑に `PromptConfig` 全体を持ち回ると整理効果が薄れます。

対策:

- dataset `match` の中で必要な prompt config を即座に取り出す
- protocol 生成以降は dataset-specific config だけを渡す

### 8.4 common runner が直接使われることで型が読みにくくなる

generic 型引数が長く見える可能性があります。

対策:

- まずは責務の明瞭さを優先する
- 可読性が問題になる場合は、runner wrapper ではなく type alias / helper 関数で対処する

## 9. 検証計画

### 9.1 維持する既存回帰

少なくとも次を維持します。

- LoCoMo の category 5 skip 挙動
- LoCoMo の retrieval judge / answerer context 共有
- LongMemEval の abstention 分離 metrics
- LongMemEval の retrieval judge / answerer context 共有
- output schema

### 9.2 追加または再配置するべき test

Phase 5.8 では次を明示します。

1. protocol の `include_question()` が LoCoMo / LongMemEval で期待どおりである test
2. protocol の `context_token_policy()` が LoCoMo / LongMemEval で期待どおりである test
3. common runner が wrapper なしでも既存 dataset 回帰を満たす test
4. CLI が使う thin composition helper 経由で protocol + common runner 組み立てでも動く test
5. dataset-specific wrapper 型に依存しない test 構成へ移せていることの確認

### 9.3 目視確認

少なくとも次を確認します。

- `rg "LoCoMoKiokuEvaluatePipeline|LongMemEvalKiokuEvaluatePipeline" crates/evaluate/src` で参照が消えていること
- `runner/mod.rs` が旧 wrapper を export していないこと
- `cli/evaluate.rs` が protocol + common runner を直接使っていること
- 新 dataset 追加時に主要追加物が protocol / prompt profile・builder 対応 / judge / config 解決であることが読み取れる構造になっていること

## 10. 実施しないこと

Phase 5.8 では次はやりません。

1. protocol trait の大幅な再設計
2. LoCoMo / LongMemEval metrics semantics の変更
3. prompt config schema の全面変更
4. judge 構築の共通化
5. 新 dataset の追加

Phase 5.8 はあくまで、
**Phase 5.7 で導入した common runner 中心設計を、wrapper を撤去してコード表面にも反映する Phase**
として扱います。
