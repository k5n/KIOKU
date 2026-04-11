# Phase 1.5 実装計画

## 1. 目的

Phase 1.5 の目的は、Phase 1 で完成した共通 runner の実行設定を **CLI 引数直書きから TOML 設定ファイルへ移行し、実験設定の再現性を確保すること** です。

この段階では LLM 連携そのものはまだ行いません。まずは次を成立させます。

1. CLI から `--config <path>` だけで評価実行できる
2. dataset / input / output_dir / backend / answerer / budget を TOML で定義できる
3. 将来の `openai-compatible` answerer に必要な設定項目を TOML に載せられる
4. 実際に使われた実験設定を `run.config.toml` と `run.resolved.json` として保存できる

## 2. Phase 1.5 の完了条件

Phase 1.5 の完了条件は次です。

1. `RunConfig` 相当の設定構造体が定義されている
2. TOML 設定ファイルを読み込み、型安全にパースできる
3. CLI は `--config <path>` のみを受ける
4. 現在の CLI 引数で渡している項目が TOML に移行されている
5. `api_key_env` を optional な設定項目として保持できる
6. 実行時に使った元設定ファイルを `run.config.toml` として output directory に保存できる
7. 解決済み設定を `run.resolved.json` として output directory に保存できる
8. `GeneratedAnswer.metadata` と `answers.jsonl.answer_metadata` は answerer 固有 metadata のみを保持し、run-level 設定を含めない
9. TOML の unknown field や inactive section は fail-fast で弾ける
10. `answer_metadata` の schema 変更を含め、run-level 設定と answerer 固有 metadata の境界が明確になっている

## 3. 前提整理

### 3.1 この変更は Phase 2 の前提整備

Phase 1.5 は LLM Answerer 導入の前提作業です。

- Phase 2 では `openai-compatible` answerer の設定項目が増える
- そのたびに CLI 引数を増やすと実験運用が重くなる
- 先に TOML 設定ファイルへ移す方が実装も実験も安定する

### 3.2 CLI override は持たない

Phase 1.5 では、CLI から設定値を部分上書きする仕組みは入れません。

- CLI は設定ファイルパスのみを受ける
- 設定の単一の truth source は TOML に置く
- 設定解決規則を複雑にしない

### 3.3 設定処理は `parse -> resolve -> validate` の 3 段階に分ける

Phase 1.5 では、設定ファイルを読み込んだ直後の生値と、実行に使う解決済み設定を分けます。

- `parse`: TOML を Rust の設定構造体へ型安全に読み込む
- `resolve`: 相対パス解決や default 適用を行い、runner が直接使う `RunConfig` を組み立てる
- `validate`: dataset / backend / answerer / retrieval の整合性を検証し、未対応設定を fail-fast で弾く

この 3 段階に分けることで、Phase 2 以降に設定項目が増えても責務を崩さず拡張できます。

加えて、再現性を優先するため次を明示します。

- TOML の typo を見逃さないよう unknown field は parse 時点で reject する
- 選択されていない backend / answerer の詳細設定は inactive section とみなし、validate で reject する
- 「書けるが無視される設定」は作らない

## 4. 実装方針

### 4.1 `RunConfig` を評価実行の単位にする

現在の `run_cli` は CLI 引数を直接 `EvaluatePipeline` に流しています。Phase 1.5 では、まず TOML から `RunConfig` を構築し、その構造体から runner の組み立てを行う形へ寄せます。

これにより次の利点があります。

- CLI 実装と評価実行ロジックを分離できる
- Phase 2 以降の answerer 設定追加を CLI 変更なしで吸収できる
- 実験設定を manifest として安定して保存しやすい

### 4.2 `ResolvedRunMetadata` は runner 側で組み立てる

Phase 1.5 では、run-level 設定の記録責務を answerer に持たせません。

- `GeneratedAnswer.metadata` は answerer 固有 metadata のみを返す
- runner は解決済み設定から `ResolvedRunMetadata` を構築する
- `ResolvedRunMetadata` は `run.resolved.json` として保存する
- `answers.jsonl.answer_metadata` は `GeneratedAnswer.metadata` をそのまま保存し、run-level 設定は merge しない
- Phase 1 までに `answer_metadata` に入っていた run-level 項目は、この段階で削除する

これにより answerer の API 境界と run provenance の正本を分離できます。

### 4.3 設定ファイル形式は TOML に統一する

Phase 1.5 では設定ファイル形式を TOML に固定します。

理由は次です。

- Rust 側のパース実装が素直
- コメントを含めた固定実験設定を書きやすい
- LoCoMo / LongMemEval ごとに別ファイルを置きやすい

### 4.4 API key は env var 名で指定し、optional にする

`openai-compatible` answerer 向けの API key 設定は、秘密値そのものではなく env var 名を TOML に書く仕様にします。

- `api_key_env = "OPENAI_API_KEY"` のように指定する
- `api_key_env` は optional とする
- 未指定時は Authorization ヘッダなしで呼び出せるようにする

これにより OpenAI 系 API と、llama.cpp / Ollama のようなローカル OpenAI 互換 API を同じ設定構造で扱えます。

## 5. 設定モデル

Phase 1.5 では、まず単一 run を表す設定ファイルを対象にします。設定ファイルは strict に扱い、各構造体は unknown field を reject する前提にします。

想定する大枠は次です。

```toml
[run]
dataset = "locomo"
input = "data/locomo10.json"
output_dir = "output/locomo"

[backend]
kind = "return-all"

[backend.return_all]

[retrieval]
max_items = 32

[answerer]
kind = "debug"
```

Phase 1.5 時点では `answerer.kind = "debug"` の実行が通れば十分です。
Phase 2 で `openai-compatible` answerer を導入するときは、その時点で `[answerer.openai_compatible]` を使う設定例を別途追加します。
`answerer.kind = "debug"` のときに `[answerer.openai_compatible]` が存在する場合は、inactive section として validate で明示的にエラーにします。

### 5.1 strict parse / validate 規則

Phase 1.5 では再現性を優先し、設定の permissive parsing は採りません。

- unknown field は parse error にする
- 選択中の `kind` に対応しない詳細設定 section は validate error にする
- `answerer.kind = "debug"` で `[answerer.openai_compatible]` がある場合は error にする
- `backend.kind = "return-all"` で将来の `[backend.kioku]` のような section がある場合も error にする

これにより typo や設定の取り違えを fail-fast で検出します。

### 5.2 パス解決規則

`run.input` と `run.output_dir` の相対パスは、**カレントディレクトリではなく設定ファイルの配置ディレクトリ基準** で解決します。

- `--config configs/locomo.toml` を渡した場合、`input = "../data/locomo10.json"` は `configs/` から見た相対パスとして解決する
- `output_dir` も同じ規則で解決する

これにより同じ設定ファイルを別の実行ディレクトリから呼んでも、同じ input / output を指せます。

### 5.3 現在の CLI 引数から移行する項目

現在 CLI で受けている次の項目は TOML へ移します。

- `dataset`
- `input`
- `output_dir`
- `backend`
- `answerer`
- `retrieval`
- `max_items`
- `max_tokens`

### 5.4 `max_tokens` の扱い

Phase 1.5 の時点では Phase 1 と同様に `ReturnAllMemoryBackend` では `max_tokens` は未対応です。

- 設定項目としては TOML に保持できる
- 未設定なら TOML 上で項目自体を省略する
- ただし Phase 1 系 backend 実装では未対応なら明示的にエラーを返す
- Phase 2 以降の answerer / backend 拡張に備えて型だけ先に固定する

`max_tokens = 0` を「未設定」の意味では使いません。

### 5.5 backend / answerer の詳細設定の持ち方

Phase 1.5 では将来の拡張で設定スキーマを壊さないため、`kind` だけでなく kind ごとの詳細設定をぶら下げられる形を先に固定します。

- backend は `[backend]` に共通項目、`[backend.<kind>]` に kind 固有設定を置く
- answerer は `[answerer]` に共通項目、`[answerer.<kind>]` に kind 固有設定を置く
- Phase 1.5 で実装するのは `backend.kind = "return-all"` と `answerer.kind = "debug"` だけでよい
- ただし schema 上は将来の `backend.oracle`、`backend.kioku`、`answerer.openai_compatible` を受けられる形にする

これにより Phase 2 以降で top-level に場当たり的な設定を増やさずに済みます。

## 6. CLI 実装計画

CLI は次の最小形へ変更します。

- `cargo run -p evaluate --bin evaluate -- --config path/to/locomo.toml`

この変更で、LoCoMo や LongMemEval の固定実験は dataset ごとに設定ファイルを分けて実行できるようにします。

## 7. 実験設定の保存

Phase 1.5 では run provenance を `answers.jsonl` に重複保存せず、output directory に別ファイルとして保存します。

- `run.config.toml`
  - ユーザが渡した元の設定ファイルを raw bytes のままコピーして保存する
  - parse 後に再 serialize して保存しない
- `run.resolved.json`
  - 実際に使った解決済み設定を保存する
  - 相対パス解決後の `input` / `output_dir`
  - default 適用後の値
  - 選択された `backend.kind` / `answerer.kind`
  - retrieval budget
  - `evaluate` crate version

加えて、`output_dir` が既存の non-empty directory を指す場合は fail-fast でエラーにします。既存 run の上書きは行いません。

run-level 設定の正本は `run.resolved.json` とします。`run.config.toml` は人間可読な入力の記録として保持します。

## 8. `answer_metadata` の扱い

Phase 1.5 では `GeneratedAnswer.metadata` と `answers.jsonl.answer_metadata` は answerer 固有 metadata のみを保持します。

- `answerer_kind`
- `mode`
- `question_id`
- `retrieved_count`

run-level 設定は `run.resolved.json` に保存し、`answers.jsonl.answer_metadata` へは merge しません。
したがって、Phase 2 で `openai-compatible` answerer を導入した後も、run 全体で不変な設定値は `answer_metadata` へ入れません。

Phase 2 で `openai-compatible` answerer を導入したら、`answer_metadata` へ追加してよいのは回答ごとに変わり得る項目だけに限定します。例えば次です。

- `request_id`
- `finish_reason`
- `usage`
- `latency_ms`

API key の実値はログへ残しません。

## 9. モジュール構成

Phase 1.5 では `crates/evaluate/src/` に次の追加を想定します。

```text
crates/evaluate/src/
├── cli/
│   ├── mod.rs
│   └── evaluate.rs
├── config/
│   ├── mod.rs
│   └── run.rs
```

CLI は「設定ファイルを読むだけ」に寄せ、設定構造体の定義と TOML 読み込みは `config/` に分離します。

## 10. 実装順序

1. `RunConfig` と関連設定型を定義する
2. TOML parse / resolve / validate を実装する
3. CLI を `--config <path>` のみに置き換える
4. runner 起動コードを `RunConfig` ベースに組み替える
5. `run.config.toml` と `run.resolved.json` の保存を実装する
6. LoCoMo / LongMemEval の実験で使える設定スキーマを文書化する

## 11. テスト計画

Phase 1.5 で最低限入れるテストは次です。

1. TOML 設定ファイルの parse test
2. unknown field を reject する test
3. 相対パスが config ファイル基準で resolve される test
4. `parse -> resolve -> validate` の境界ごとの test
5. inactive section を reject する test
6. `--config` 必須化の CLI test
7. `RunConfig` から debug answerer 実行まで通る test
8. `run.config.toml` が raw bytes copy で保存される test
9. `run.resolved.json` に crate version が入る test
10. 既存 non-empty `output_dir` を reject する test
11. `answers.jsonl.answer_metadata` に run-level 設定が混入しない test
12. `api_key_env = None` を許容する test

## 12. リスクと対策

### 12.1 設定項目を増やし過ぎるリスク

対策:

- まずは単一 run 用の最小構造に留める
- matrix 実行や複数 experiment の一括定義は後回しにする

### 12.2 Phase 1 と Phase 2 の責務が混ざるリスク

対策:

- Phase 1.5 では設定基盤だけを対象にする
- `openai-compatible` answerer の実装そのものは Phase 2 に残す

### 12.3 permissive parsing で設定ミスを見逃すリスク

対策:

- unknown field は parse error にする
- inactive section は validate error にする
- run provenance の正本は `run.resolved.json` に一本化する

### 12.4 output 上書きで run provenance を壊すリスク

対策:

- 既存 non-empty `output_dir` は fail-fast で弾く
- `run.config.toml` と `run.resolved.json` を固定名で安全に保存できる前提を守る

## 13. Phase 1.5 完了後に着手するもの

Phase 1.5 の次に進める対象は次です。

1. `LlmAnswerer` trait の定義
2. prompt builder の実装
3. `LlmBackedAnswerer<T>` の実装
4. `RigOpenAiCompatibleLlmAnswerer` の実装
