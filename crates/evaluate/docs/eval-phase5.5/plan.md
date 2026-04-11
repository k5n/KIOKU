# Phase 5.5 実施計画

## 1. 目的

Phase 5.5 の目的は、Phase 5 で `longmemeval_kioku_v1` への移行が完了したあとも  
`crates/evaluate` に残っている **旧 LongMemEval 評価仕様の実装を完全に削除すること** です。

ここで削除したいのは、単に未使用ファイルを消すことではありません。  
LongMemEval の実行・設定・prompt・judge・metrics・artifact schema に残っている  
旧仕様の入口と分岐をなくし、**LongMemEval は `longmemeval_kioku_v1` のみを唯一の正規 path とする** 状態まで整理します。

Phase 5 が LongMemEval を KIOKU 用 protocol へ移行する Phase だったのに対し、  
Phase 5.5 は **移行後に残った provisional / legacy 実装を撤去して保守面を正常化する Phase** です。

## 2. 前提

Phase 5 完了時点で、LongMemEval の実行 path はすでに `longmemeval_kioku_v1` を使えます。  
しかしコードベース上には次の旧実装が残っています。

- `Judge` trait ベースの `LongMemEvalJudge`
- `[prompt.longmemeval]` と `LongMemEvalPromptConfig`
- `LongMemEvalAnswerPromptProfile` による profile 切替
- `longmemeval.answer.*` 系 template ID を返す prompt builder
- `requested_longmemeval_prompt_profile` を持つ backend query I/F
- 旧 exact-match metrics / output schema を前提にした test
- `return-all` backend の legacy profile 分岐
- `EvaluatePipeline` 側に残った LongMemEval legacy test path

実行系は新仕様に寄っていても、これらが残っていると次の問題があります。

1. LongMemEval の正規 path がコード上で一意に見えない
2. prompt / backend / metrics の意味論が二重化する
3. 未使用コードの保守コストが継続する
4. 将来の変更時に誤って旧 path を直してしまう危険がある

## 3. Phase 5.5 の方針

### 3.1 後方互換は持たない

Phase 5.5 では **後方互換性を意図的に無視** します。

具体的には次を許容します。

- 古い設定ファイルの `[prompt.longmemeval]` は読み込めなくなる
- `longmemeval.answer.*` を前提にした test / script は壊れる
- `longmemeval_exact_match` を前提にした metrics / artifact expectation は壊れる
- 旧 LongMemEval 実装に依存する internal API は削除される

LongMemEval は `longmemeval_kioku_v1` へ完全移行済みという前提で、  
**削除によって設定や API が単純になることを優先** します。

### 3.2 LongMemEval の正規構成を 1 本に固定する

Phase 5.5 後の LongMemEval は次の構成だけを持ちます。

- prompt 設定:
  - `[prompt.longmemeval_kioku]`
- answer prompt:
  - `longmemeval.kioku.answer.v1`
- answer judge:
  - `LongMemEvalKiokuAnswerJudge`
- retrieval judge:
  - `LongMemEvalKiokuRetrievalJudge`
- pipeline:
  - `LongMemEvalKiokuEvaluatePipeline`
- metrics semantics:
  - `longmemeval_kioku_v1`

これ以外の LongMemEval 向け分岐は残しません。

### 3.3 削除は「入口から内部へ」ではなく「依存の末端から入口へ」進める

旧仕様の削除は、設定だけ先に消すと中間状態でビルドが大きく壊れやすいです。  
そのため、次の順で進めます。

1. 旧 test と旧参照を洗い出す
2. 旧 prompt / judge / metrics の末端実装を削る
3. backend / model I/F の legacy field を削る
4. config / metadata から legacy field を削る
5. 最後に module export と doc を整理する

## 4. 完了条件

Phase 5.5 の完了条件は次です。

1. `LongMemEvalJudge` がコードベースから削除されている
2. `LongMemEvalAnswerPromptProfile` と `LongMemEvalPromptConfig` が削除されている
3. `[prompt.longmemeval]` を受け取る TOML schema / resolve / validate path が削除されている
4. LongMemEval 用 answer prompt は `longmemeval.kioku.answer.v1` のみになる
5. `QueryInput.requested_longmemeval_prompt_profile` が削除されている
6. `return-all` backend にある LongMemEval legacy profile 分岐が削除されている
7. `longmemeval.answer.*` と `longmemeval_exact_match` を前提にした test が削除または置換されている
8. `run.resolved.json` に旧 LongMemEval prompt metadata が出力されなくなる
9. LongMemEval の artifact / metrics / output test は `longmemeval_kioku_v1` のみを前提に通る
10. LoCoMo の `locomo_kioku_v1` path は Phase 5.5 の削除で壊れない

## 5. 削除対象

### 5.1 judge

削除対象:

- `crates/evaluate/src/judge/longmemeval.rs`
- `crates/evaluate/src/judge/mod.rs` の `mod longmemeval;`
- `crates/evaluate/src/judge/mod.rs` の `pub use longmemeval::LongMemEvalJudge;`

期待する状態:

- LongMemEval 向け judge は `AnswerJudge` / `RetrievalJudge` ベース実装だけになる
- `Judge` trait を使う LongMemEval path は存在しない

### 5.2 prompt

削除対象:

- `LongMemEvalAnswerPromptProfile`
- `LongMemEvalPromptConfig`
- `build_legacy_longmemeval_prompt`
- `resolve_longmemeval_context`
- `prompt/profiles/longmemeval.rs`
- `prompt/mod.rs` の legacy export

期待する状態:

- `PromptBuildRequest` から legacy LongMemEval prompt 用 field が消える
- LongMemEval の prompt builder は `longmemeval_kioku_prompt` 前提だけになる
- `longmemeval.answer.no_retrieval.v1` などの template ID はコードから消える

### 5.3 backend / model

削除対象:

- `QueryInput.requested_longmemeval_prompt_profile`
- `ReturnAllMemoryBackend` の profile 別分岐
- facts-only / no-retrieval / history-chats-with-facts を前提にした validation

期待する状態:

- LongMemEval query は dataset / question / budget / metadata のみで実行できる
- backend は LongMemEval では常に deterministic な `prompt_context` を返す

### 5.4 config / metadata

削除対象:

- `TomlPromptSection.longmemeval`
- `TomlLongMemEvalPromptSection`
- `PromptConfig.longmemeval`
- `ResolvedPromptMetadata.longmemeval`
- `resolve_prompt_config` の legacy LongMemEval resolve 分岐
- `resolved_metadata()` の legacy LongMemEval metadata 出力
- legacy config parse / resolve test

期待する状態:

- LongMemEval 設定は `[prompt.longmemeval_kioku]` のみ
- resolved metadata には `longmemeval_kioku` だけが残る

### 5.5 runner / metrics / outputs

削除対象:

- `EvaluatePipeline` に残る LongMemEval legacy test
- `build_metrics` の LongMemEval legacy semantics 依存 test
- `longmemeval_exact_match` を前提にした provenance expectation
- `runner/output.rs` にある旧 LongMemEval artifact schema test

整理対象:

- `EvaluatePipeline` 自体を残すかどうかはこの Phase で判断する

判断基準:

- もし本番 path が LoCoMo / LongMemEval とも dataset-specific pipeline のみなら、
  共通 `EvaluatePipeline` は `EvaluatePipelineResult` を残して撤去する方が自然
- ただし LoCoMo 側でまだ汎用部品として意味があるなら、LongMemEval 依存だけ抜く

## 6. 実装手順

### Step 1. 旧 LongMemEval prompt 実装を削除する

まず prompt 周りから旧仕様を撤去します。

実施内容:

1. `prompt/answer.rs` から legacy LongMemEval profile enum / config / builder を削除する
2. `PromptBuildRequest` から `longmemeval_prompt` を削除する
3. `prompt/profiles/longmemeval.rs` を削除する
4. `prompt/mod.rs` の export を更新する
5. `answerer/llm.rs` の旧 template id 前提 test を Kioku prompt 前提へ更新する

この Step の目的は、LongMemEval prompt の意味論を  
**Kioku prompt 1 本に固定すること** です。

### Step 2. 旧 judge 実装を削除する

次に judge 側の provisional path を削除します。

実施内容:

1. `judge/longmemeval.rs` を削除する
2. `judge/mod.rs` から legacy module / re-export を削除する
3. `runner/pipeline.rs` や test から `LongMemEvalJudge` 参照を除去する

この Step の目的は、LongMemEval の採点経路を  
**dual-judge path だけにすること** です。

### Step 3. backend query I/F から legacy profile を外す

次に backend と query model を簡略化します。

実施内容:

1. `model/retrieval.rs` から `requested_longmemeval_prompt_profile` を削除する
2. `runner/longmemeval_kioku.rs` と `runner/locomo_kioku.rs` の `QueryInput` 構築を更新する
3. `backend/return_all.rs` の LongMemEval legacy profile 分岐を削除する
4. `return-all` backend test を新仕様前提へ更新する

この Step の目的は、  
**backend query I/F から旧 prompt 意味論を追い出すこと** です。

### Step 4. config / metadata から旧 LongMemEval 設定を削除する

ここで設定レイヤからも legacy 入口を消します。

実施内容:

1. `config/toml.rs` から `[prompt.longmemeval]` schema を削除する
2. `config/types.rs` から `PromptConfig.longmemeval` と `ResolvedPromptMetadata.longmemeval` を削除する
3. `config/resolve.rs` から legacy resolve 分岐と関連 test を削除する
4. `config/validate.rs` の「inactive prompt section `[prompt.longmemeval]`」判定を削除または整理する
5. `config/metadata.rs` の resolved metadata を更新する

この Step の目的は、  
**設定上も LongMemEval の正規 path を 1 本にすること** です。

### Step 5. runner / metrics / output の旧テストを撤去する

最後に artifact と metrics 周辺の整理を行います。

実施内容:

1. `runner/output.rs` の旧 LongMemEval artifact schema test を削除する
2. `runner/metrics.rs` の `longmemeval_exact_match` 前提 test を削除する
3. `runner/pipeline.rs` の LongMemEval legacy test を削除する
4. 必要なら `EvaluatePipeline` の役割を再評価し、不要なら削除する

この Step の目的は、  
**テスト suite 上でも旧仕様の存在をなくすこと** です。

## 7. 検証計画

Phase 5.5 の検証は次で行います。

### 7.1 参照削除確認

少なくとも次の文字列が production code から消えていることを確認します。

- `LongMemEvalJudge`
- `LongMemEvalAnswerPromptProfile`
- `LongMemEvalPromptConfig`
- `requested_longmemeval_prompt_profile`
- `longmemeval.answer.`
- `longmemeval_exact_match`

### 7.2 config / build / test

少なくとも次を通します。

1. `cargo fmt --all`
2. `cargo test -p evaluate`
3. 必要なら `cargo check`

### 7.3 LongMemEval artifact semantics

LongMemEval 実行相当の test で次を確認します。

1. `run.resolved.json` に `prompt.longmemeval` が出ない
2. `answers.jsonl` の `answer_metadata.template_id` が `longmemeval.kioku.answer.v1` である
3. `metrics.json.protocol` が `longmemeval_kioku_v1` である
4. retrieval / answer judge provenance が保持される

## 8. 影響範囲と注意点

### 8.1 破壊的変更

この Phase は意図的に破壊的です。

影響:

- 旧 config は parse / validate に失敗する
- 旧 test fixture はそのままでは使えない
- 旧 artifact schema を前提にした補助スクリプトがあれば更新が必要

### 8.2 doc の整合

実装後は少なくとも次の文書の記述を確認します。

- `doc/implement-evaluation-plan.md`
- `doc/eval-phase3/plan.md`
- `doc/eval-phase5/plan.md`
- `doc/KIOKU-LongMemEval-evaludation.md`

Phase 履歴として旧仕様を残すのはよいですが、  
**現行実装がまだ旧 path を持つように読める記述** は整理が必要です。

## 9. この Phase でやらないこと

Phase 5.5 では次は扱いません。

- LongMemEval judge rubric 自体の再設計
- `longmemeval_kioku_v2` の導入
- tokenizer 実装の高度化
- `KiokuMemoryBackend` の本実装
- LoCoMo metrics / prompt semantics の再設計

## 10. 実施順序の要約

推奨順は次です。

1. prompt legacy 実装を削除する
2. judge legacy 実装を削除する
3. backend / model I/F の legacy field を削除する
4. config / metadata の legacy 入口を削除する
5. runner / metrics / output の旧 test を削除する
6. doc を同期する

この順にすると、LongMemEval の正規 path を維持したまま  
依存の深いところから安全に legacy 実装を削れます。
