# `crates/evaluate` リファクタリング計画

## 1. 結論

現状の問題意識は妥当です。特に次の 2 点は早めに整理した方がよいです。

1. `LoCoMo` / `LongMemEval` 専用ロジックが `datasets` / `prompt` / `judge` / `runner` / `config` / `cli` に横断して散っている
2. `mod.rs` と `lib.rs` が公開面を整理するより、むしろ benchmark 固有型を広く再 export してしまっている

ただし、最終形は `src/core/*, src/locomo/*, src/longmemeval/*` よりも、次の構成を推奨します。

- `src/common/*`
- `src/benchmarks/locomo/*`
- `src/benchmarks/longmemeval/*`

理由は次です。

- `crates/core` という workspace crate がすでに存在するため、`src/core` は名前として紛らわしい
- `LoCoMo` / `LongMemEval` はプロダクト本体の domain ではなく benchmark 実装であることを明示したい
- 「共通」と「benchmark 専用」の境界を、ディレクトリ名だけで説明できる

この計画では、benchmark 固有コードを benchmark module 配下へ集約し、共通 runner / backend / answerer / model / token counter は `common` 側へ残します。一方、`config` は入力形式と benchmark 選択を扱う orchestration 層として top-level に残します。そのうえで `lib.rs` と各 `mod.rs` は「公開面を絞る場所」として再定義します。

## 2. 現状の課題

### 2.1 benchmark 固有コードが横断配置されている

現在の `LoCoMo` / `LongMemEval` 専用実装は、少なくとも次に分散しています。

- `src/datasets/locomo.rs`
- `src/datasets/longmemeval.rs`
- `src/prompt/answer.rs`
- `src/prompt/profiles/locomo.rs`
- `src/judge/locomo_kioku.rs`
- `src/judge/longmemeval_kioku.rs`
- `src/runner/protocol/locomo.rs`
- `src/runner/protocol/longmemeval.rs`
- `src/runner/metrics.rs`
- `src/config/resolve.rs`
- `src/config/validate.rs`
- `src/config/metadata.rs`
- `src/cli/evaluate.rs`

この配置だと、「LoCoMo の仕様を追いたい」と思っても 1 箇所を見れば済まず、変更差分の見通しが悪くなります。

### 2.2 共通 module が benchmark 名を知りすぎている

現在の共通 module は、本来 benchmark 非依存であるべき位置に benchmark 名が入り込んでいます。

- `prompt::AnswerPromptProfile` が `LoCoMoKioku` / `LongMemEvalKioku` を直接持っている
- `DefaultPromptBuilder` が benchmark ごとの prompt 構築分岐を持っている
- `runner::metrics` が benchmark ごとの集計ロジックを持っている
- `judge/mod.rs` が共通 trait と benchmark 実装を同じ階層で export している

この状態では「共通 module を見れば benchmark 非依存の contract が分かる」という構図になっていません。

### 2.3 型が無効状態を許している

現在の `PromptConfig` は次の形です。

```rust
pub struct PromptConfig {
    pub longmemeval_kioku: Option<LongMemEvalKiokuPromptConfig>,
    pub locomo_kioku: Option<LocomoKiokuPromptConfig>,
}
```

この形だと、型レベルでは次の無効状態を許します。

- 両方 `None`
- 両方 `Some`
- `run.dataset = "locomo"` なのに `longmemeval_kioku` が `Some`

さらに現在の入力 config も、`run.dataset` と top-level の `[prompt.*]` section が分離しているため、同じ無効状態を TOML 上でそのまま表現できてしまいます。
そのため `validate.rs` に benchmark ごとの否定条件が増えています。これは構造上の問題です。

### 2.4 `mod.rs` / `lib.rs` の公開面が広すぎる

現在の公開構造には次の問題があります。

- `src/lib.rs` が top-level module をすべて `pub mod` で開いている
- `src/model/mod.rs` は `pub mod ...` と `pub use ...` を併用しており、公開経路が二重になっている
- `src/prompt/profiles/mod.rs` は LoCoMo だけを公開しており、構成が非対称
- `src/judge/mod.rs` / `src/datasets/mod.rs` が benchmark 固有型を上位に平坦化している

特に現状の repo 内で `evaluate` crate を外部から使っている箇所を確認すると、実質的には `src/bin/evaluate.rs` から `evaluate::cli::{Cli, run_cli}` を使っているだけです。この状態で広い public API を維持する理由は薄いです。

### 2.5 役割不明な要素が残っている

少なくとも次は整理対象です。

- `judge::Judge` trait は現状未使用
- `prompt/profiles/locomo.rs` は system prompt 定数だけのため、module 境界として弱い
- LongMemEval 側には `prompt/profiles/longmemeval.rs` がなく、配置方針が揃っていない

## 3. リファクタリング原則

今回のリファクタリングでは、次の原則を守ります。

- benchmark 固有ロジックは benchmark module 配下に閉じ込める
- 共通 module は `LoCoMo` / `LongMemEval` という固有名詞を原則として持たない
- benchmark 間で共有するのは「正規化済みの評価用 contract」だけにする
- 入力 config でも benchmark 選択と benchmark 固有設定を同じ section にまとめる
- public API は最小化し、`pub(crate)` をデフォルトにする
- `run.resolved.json` / `answers.jsonl` / `retrieval.jsonl` / `metrics.json` の schema、protocol id、prompt id、metrics semantics は変えない
- 構造整理が主目的であり、評価ロジックの意味変更は避ける

### 3.1 benchmark 境界を越えてよい共通 contract

benchmark module と共通 module の間でやり取りしてよいのは、原則として次だけに絞ります。

- `BenchmarkCase`
- `BenchmarkEvent`
- `BenchmarkQuestion`
- `PreparedPrompt`
- `PromptContext`
- `PromptBuilder`
- `BinaryJudgement`
- `MetricsReport`
- `MemoryBackend`
- `Answerer`
- `TokenCounter`

逆に、次は benchmark module 内に閉じ込めます。

- raw dataset 型
- benchmark 固有 prompt config
- benchmark 固有 judge 実装
- benchmark 固有 protocol 実装
- benchmark 固有 metrics 集計
- benchmark 固有 config validation / metadata 補完

## 4. 推奨最終構成

最終的な構成は次を推奨します。

```text
crates/evaluate/src/
├── lib.rs
├── cli/
│   ├── mod.rs
│   └── evaluate.rs
├── config/
│   ├── mod.rs
│   ├── metadata.rs
│   ├── resolve.rs
│   ├── toml.rs
│   ├── types.rs
│   └── validate.rs
├── common/
│   ├── mod.rs
│   ├── answerer/
│   │   ├── mod.rs
│   │   ├── debug.rs
│   │   ├── llm.rs
│   │   └── rig_openai.rs
│   ├── backend/
│   │   ├── mod.rs
│   │   ├── return_all.rs
│   │   └── traits.rs
│   ├── judge/
│   │   ├── mod.rs
│   │   ├── runtime.rs
│   │   └── traits.rs
│   ├── model/
│   │   ├── mod.rs
│   │   ├── answer.rs
│   │   ├── benchmark.rs
│   │   ├── metrics.rs
│   │   └── retrieval.rs
│   ├── prompt/
│   │   ├── mod.rs
│   │   ├── builder.rs
│   │   ├── context.rs
│   │   └── prepared.rs
│   ├── runner/
│   │   ├── mod.rs
│   │   ├── helpers.rs
│   │   ├── output.rs
│   │   ├── pipeline.rs
│   │   ├── policy.rs
│   │   └── result.rs
│   └── token_counter.rs
└── benchmarks/
    ├── mod.rs
    ├── locomo/
    │   ├── mod.rs
    │   ├── config.rs
    │   ├── dataset.rs
    │   ├── judge.rs
    │   ├── metrics.rs
    │   ├── prompt.rs
    │   └── protocol.rs
    └── longmemeval/
        ├── mod.rs
        ├── config.rs
        ├── dataset.rs
        ├── judge.rs
        ├── metrics.rs
        ├── prompt.rs
        └── protocol.rs
```

## 5. 現在ファイルからの移行先

| 現在 | 移行先 | 備考 |
| --- | --- | --- |
| `src/datasets/locomo.rs` | `src/benchmarks/locomo/dataset.rs` | raw loader と `BenchmarkCase` adapter を同居 |
| `src/datasets/longmemeval.rs` | `src/benchmarks/longmemeval/dataset.rs` | raw loader と `BenchmarkCase` adapter を同居 |
| `src/prompt/answer.rs` の LoCoMo 部分 | `src/benchmarks/locomo/prompt.rs` | LoCoMo prompt config と build 関数を移す |
| `src/prompt/answer.rs` の LongMemEval 部分 | `src/benchmarks/longmemeval/prompt.rs` | LongMemEval prompt config と build 関数を移す |
| `src/prompt/profiles/locomo.rs` | `src/benchmarks/locomo/prompt.rs` | system prompt 定数を統合 |
| `src/judge/locomo_kioku.rs` | `src/benchmarks/locomo/judge.rs` | benchmark 固有 judge |
| `src/judge/longmemeval_kioku.rs` | `src/benchmarks/longmemeval/judge.rs` | benchmark 固有 judge |
| `src/runner/protocol/locomo.rs` | `src/benchmarks/locomo/protocol.rs` | question filter / metric input 構築を保持 |
| `src/runner/protocol/longmemeval.rs` | `src/benchmarks/longmemeval/protocol.rs` | context token 要件もここに寄せる |
| `src/runner/metrics.rs` の LoCoMo 部分 | `src/benchmarks/locomo/metrics.rs` | benchmark 固有集計 |
| `src/runner/metrics.rs` の LongMemEval 部分 | `src/benchmarks/longmemeval/metrics.rs` | benchmark 固有集計 |
| `src/config/toml.rs` の `[prompt.*]` schema | `src/config/toml.rs` + `src/benchmarks/*/config.rs` | top-level `prompt` を廃止し、`[benchmark.<name>]` 形式へ変更 |
| `src/config/*` の benchmark 分岐 | `src/config/*` + `src/benchmarks/*/config.rs` | top-level `config` は orchestration を担当し、benchmark 固有 resolve / validate / metadata 補完を benchmark module へ委譲する |
| `src/cli/evaluate.rs` の benchmark match | `src/cli/evaluate.rs` | orchestration 層で benchmark 分岐を 1 箇所に集約する |

## 6. 重要な設計変更

### 6.1 benchmark config を型安全にする

現在の問題は、内部の `PromptConfig` だけでなく、入力 config でも `run.dataset` と `[prompt.*]` が分離していることです。
したがって、内部型と入力 TOML の両方をまとめて変えます。

新しい入力 config は、top-level `prompt` section を廃止し、benchmark 選択と benchmark 固有設定を同じ section に持たせます。例えば次です。

```toml
[run]
input = "data/locomo.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"

[judge]
kind = "openai-compatible"

[judge.openai-compatible]
base_url = "http://localhost:11434/v1"
model = "judge-model"
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 512
timeout_secs = 60
max_retries = 3
retry_backoff_ms = 500

[benchmark.locomo]
answer_template_id = "locomo.kioku.answer.v1"
answer_judge_prompt_id = "locomo.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "locomo.kioku.judge.retrieval.v1"
```

```toml
[run]
input = "data/longmemeval.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"

[judge]
kind = "openai-compatible"

[judge.openai-compatible]
base_url = "http://localhost:11434/v1"
model = "judge-model"
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 512
timeout_secs = 60
max_retries = 3
retry_backoff_ms = 500

[benchmark.longmemeval]
answer_template_id = "longmemeval.kioku.answer.v1"
answer_judge_prompt_id = "longmemeval.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "longmemeval.kioku.judge.retrieval.v1"
```

内部の `RunConfig` も、それに対応する benchmark 別 enum に変えます。

```rust
pub struct RunConfig {
    pub input: PathBuf,
    pub output_dir: PathBuf,
    pub backend: BackendConfig,
    pub answerer: AnswererConfig,
    pub judge: Option<JudgeConfig>,
    pub retrieval: RetrievalBudget,
    pub benchmark: BenchmarkConfig,
}

pub enum BenchmarkConfig {
    LoCoMo(locomo::BenchmarkConfig),
    LongMemEval(longmemeval::BenchmarkConfig),
}
```

各 benchmark 側 config は、自分に必要な prompt config を必須で持ちます。

```rust
pub struct LoCoMoBenchmarkConfig {
    pub prompt: LoCoMoKiokuPromptConfig,
}
```

ここでの benchmark config や prompt config は、**参照ではなく所有で扱う**前提にします。
少なくとも現状の `LocomoKiokuPromptConfig` / `LongMemEvalKiokuPromptConfig` は prompt id を表す少数の `String` からなる value object なので、`protocol` や benchmark bundle が clone して所有して問題ありません。むしろ borrow にすると、`prepare_run(...)` が返す bundle の中で「config を所有しつつ protocol がそれを参照する」形になりやすく、不要な lifetime 制約や自己参照に近い構造を招きます。

したがって、benchmark module 間の所有権ルールは次で統一します。

- benchmark config は prompt config を所有する
- benchmark protocol は必要な prompt config を所有する
- benchmark judge は必要な prompt id を `String` として所有する
- `PreparedBenchmarkRun` は自己参照を含まない所有型にする

`run.dataset` と `[prompt.*]` を別々に持つ構造は廃止します。
旧 config 形式との互換レイヤは作らず、parser は新しい `[benchmark.<name>]` 形式だけを受け付ける方針にします。

さらに benchmark section は「0 個または 2 個以上」を許さず、**config load の validation 段階でちょうど 1 つだけ存在する**ことを強制します。
具体的には `toml.rs` 側では benchmark 入力を wrapper struct として受けます。例えば `TomlBenchmarkSection { locomo: Option<...>, longmemeval: Option<...> }` のように deserialize 自体は許容し、`validate.rs` 側で `[benchmark.locomo]` または `[benchmark.longmemeval]` のどちらか 1 つだけが存在することを検証します。これにより、benchmark 未指定・複数同時指定の両方を config error として reject できます。

これにより、次が不要になります。

- `run.dataset` と `prompt.*` の整合性チェック
- inactive prompt section の手動 reject
- `PromptConfig` の `Option` 2 本構成を前提にした resolve / validate 分岐

一方で、`run.resolved.json` / `answers.jsonl` / `retrieval.jsonl` / `metrics.json` の schema は維持するため、`metadata.rs` では `BenchmarkConfig` から既存 schema への projection を行います。
`run.config.toml` は raw input copy なので、新しい入力形式に合わせて内容が変わります。つまり「入力 config 形式と `run.config.toml` の内容は変わるが、解決済み設定と評価結果 artifact の schema は壊さない」という方針です。

また、`config` module は `common` には入れません。`common` は benchmark 非依存の実行部品だけを置く層であり、入力形式と benchmark 選択を扱う `config` は top-level の orchestration 層として残します。

### 6.2 共通 `prompt` から benchmark 分岐を外す

現在の `AnswerPromptProfile` enum と `DefaultPromptBuilder` は、共通 module が benchmark 名を知る原因です。これをやめます。

推奨は次です。

- 共通側 `prompt` には `PreparedPrompt` / `PromptContext` / `PromptBuilder` trait を残す
- `AnswerPromptProfile` は削除する
- `DefaultPromptBuilder` は削除する
- `benchmarks/locomo/prompt.rs` と `benchmarks/longmemeval/prompt.rs` に、それぞれの `PromptBuilder` 実装を置く
- `run_pipeline(...)` は引き続き `P: PromptBuilder` を受け取る
- orchestration 層は benchmark ごとに整合した `prompt_builder` を受け取って `run_pipeline(...)` へ渡す

共通 runner が必要とするのは「最終的に `PreparedPrompt` が得られること」だけです。したがって、共通層に残すべきなのは benchmark enum を内包した default 実装ではなく、「prompt を構築できる」という trait contract です。

変更後の責務イメージ:

- 共通 runner
  - `query`
  - retrieval judge 実行
  - `PromptBuilder` を通じて prompt を構築して answerer 実行
  - log / metrics の共通組み立て
- benchmark prompt builder
  - benchmark 固有 prompt config を受け取る
  - benchmark 固有 answer prompt を構築する
- benchmark protocol
  - question filter
  - metric input 構築
  - metrics 集計

### 6.3 共通 `judge` から benchmark 実装を外す

共通 `judge` 側に残すのは次だけでよいです。

- `BinaryJudgement`
- `AnswerJudge`
- `RetrievalJudge`
- `OpenAiCompatibleJudgeRuntime`

`LoCoMoKiokuAnswerJudge` などは benchmark module 側へ移し、`judge/mod.rs` からの benchmark 実装 re-export は廃止します。

また、未使用の `Judge` trait は削除候補です。

### 6.4 orchestration 層に benchmark dispatch を集約する

benchmark ごとの `match` は 1 箇所に集約します。

現状は次に分散しています。

- case load
- prompt resolve / validate
- judge build
- protocol build
- token counter 選択

ただし、`benchmarks/mod.rs` 自体を唯一の dispatch 点にはしません。

- `benchmarks/*` は benchmark 固有の実行準備をまとめた bundle を返す
- `common/runner` は共通 pipeline を提供する
- `cli/evaluate.rs` のような orchestration 層が、benchmark module から返された bundle を thin helper 経由で `run_pipeline` に渡す

つまり、branching の集約先は benchmark namespace ではなく、「pipeline 実行を指揮する層」です。

重要なのは、「benchmark-specific branching をゼロにする」ことではなく、「branching の置き場を 1 箇所に固定する」ことです。

さらに、benchmark module から orchestration 層へは、個別部品ではなく「一緒に動く部品の束」を返す形を推奨します。

例えば次のような `PreparedBenchmarkRun` を返します。

```rust
pub(crate) struct PreparedBenchmarkRun<PB, P, AJ, RJ> {
    pub cases: Vec<BenchmarkCase>,
    pub prompt_builder: PB,
    pub protocol: P,
    pub answer_judge: AJ,
    pub retrieval_judge: RJ,
    pub token_counter: Option<Box<dyn TokenCounter>>,
}
```

このとき `PB` / `P` / `AJ` / `RJ` は、benchmark config の一部を borrow する前提にしません。
特に prompt builder と protocol は `&LocomoKiokuPromptConfig` のような参照を保持するのではなく、必要な prompt config を clone して所有する形を推奨します。judge も同様に、必要な prompt id を借用ではなく `String` として保持します。これにより `prepare_run(...)` が返す bundle を、lifetime parameter なしの通常の所有型として組み立てられます。

この方針にすると、次の結び付き条件を benchmark module 内へ閉じ込められます。

- `load_cases` した dataset と `protocol.dataset()` の一致
- `prompt_builder` が生成する prompt と `protocol` が想定する metric / prompt id の一致
- judge が参照する prompt id と protocol の prompt config の一致
- `token_counter` の有無と `context_token_policy()` の一致
- LoCoMo / LongMemEval ごとの将来追加要件

逆に、`build_prompt_builder` / `build_protocol` / `build_judges` / `build_token_counter` を個別に export すると、orchestration 層がそれらの組み立て順と整合条件を知り続ける必要があります。

したがって、benchmark module の公開面は「細かい builder 群」ではなく、`prepare_run(...) -> PreparedBenchmarkRun<_>` のような entry point に寄せるのを推奨します。内部では `load_cases` や `build_judges` などを private helper として持って構いません。

ここで重要なのは、`PreparedBenchmarkRun` 自体を `run_pipeline` の引数へ直接昇格させないことです。`run_pipeline` は引き続き common runner の低レベル primitive として保ち、bundle の展開は orchestration 側の thin helper が担います。

例えば次のような責務分担を想定します。

- `prepare_run(...)`
  - benchmark 固有の bundle を組み立てる
- `execute_prepared_run(...)`
  - `PreparedBenchmarkRun` を展開し、`prompt_builder` / `protocol` / `judges` / `token_counter` を `run_pipeline(...)` へ橋渡しするだけの thin helper
- `run_pipeline(...)`
  - common runner の primitive として、stub prompt builder / stub protocol / stub judge を直接差し込める形を維持する

この形なら、`match benchmark` は orchestration 層の 1 箇所に残しつつ、`run_pipeline(...)` の引数構造は必要以上に benchmark 側の都合へ引きずられません。また、`PreparedBenchmarkRun<PB, P, AJ, RJ>` の具体型は各 `match` arm の中で完結し、trait object 化や enum wrapper 化を前提にしなくて済みます。

なお、この設計では「borrow でコピーを避ける」ことは優先しません。`protocol` や `judge` が保持するのは少数の prompt id 文字列であり、run ごとに clone するコストは小さい一方、借用を残すと API 全体に lifetime が漏れやすくなります。今回の目的は構造整理と境界の明確化なので、ここでは clone による所有を優先します。

## 7. `mod.rs` / 公開面のルール

### 7.1 基本ルール

- `pub` は crate 外へ約束したい API に限定する
- それ以外は `pub(crate)` をデフォルトにする
- leaf module は private をデフォルトにする
- `mod.rs` は directory boundary の facade に限定して使う

### 7.2 `lib.rs` の方針

`src/lib.rs` は `pub mod ...` をやめ、原則として次のようにするのを推奨します。

```rust
mod benchmarks;
mod cli;
mod common;
mod config;

pub use cli::{Cli, run_cli};
```

追加で public にするものが本当に必要になったら、その時点で明示的に `pub use` を足します。

現時点では repo 内利用が `Cli` / `run_cli` にほぼ限られているため、最小公開に寄せる方が安全です。

### 7.3 submodule facade のルール

`benchmarks/locomo/mod.rs` や `benchmarks/longmemeval/mod.rs` は、親 module が必要とする entry point のみを export します。例えば次です。

- `BenchmarkConfig`
- `resolve_config`
- `validate_config`
- `prepare_run`

ここでの `prepare_run` は、cases / protocol / judges / token counter を含む `PreparedBenchmarkRun` を返す entry point です。

benchmark module 自身が共通 pipeline 実行まで引き受ける必要はありません。共通 pipeline の実行は引き続き orchestration 層の責務です。

逆に export しないもの:

- raw parse helper
- prompt text の細かい helper
- rubric 文字列の helper
- test support

### 7.4 二重公開を禁止する

次の形は避けます。

```rust
pub mod benchmark;
pub use benchmark::BenchmarkCase;
```

理由は、利用者が `crate::model::benchmark::BenchmarkCase` と `crate::model::BenchmarkCase` の両方を使えてしまい、公開面が不安定になるためです。

必要なら次のどちらかに寄せます。

- module 自体を公開しないで `pub use` のみ残す
- module 自体を公開し、re-export はしない

今回の crate では前者を推奨します。

## 8. 詳細実施順

### Phase A: benchmark namespace の導入

目的:

- benchmark 固有コードの置き場所を先に作る

作業:

- `src/benchmarks/mod.rs` を追加
- `src/benchmarks/locomo/mod.rs` を追加
- `src/benchmarks/longmemeval/mod.rs` を追加
- まずは新 module から旧 module を内部 re-export する薄い facade を作る
- `benchmarks/mod.rs` は dispatch の司令塔ではなく、namespace 境界として導入する

完了条件:

- 新 namespace から既存機能に到達できる
- 振る舞いはまだ変えない

### Phase B: dataset / prompt / judge / protocol / metrics の物理移動

目的:

- benchmark ごとの関心を同じディレクトリに集める

作業:

- `datasets/*.rs` を各 benchmark の `dataset.rs` へ移動
- `judge/*.rs` の benchmark 実装を各 benchmark の `judge.rs` へ移動
- `runner/protocol/*.rs` を各 benchmark の `protocol.rs` へ移動
- `runner/metrics.rs` を benchmark 別 `metrics.rs` へ分割
- `prompt/answer.rs` の benchmark 分岐を分解し、各 benchmark の `prompt.rs` へ移動

完了条件:

- `src/runner` / `src/prompt` / `src/judge` から benchmark 固有ファイルがほぼ消える
- benchmark module を見れば、その benchmark の実装全体を追える

### Phase C: config 入力と型の再設計

目的:

- 無効状態を入力 config と内部型の両方で表せないようにする

作業:

- top-level `prompt` section を廃止する
- `run.dataset` を廃止し、benchmark 選択を `[benchmark.locomo]` / `[benchmark.longmemeval]` に移す
- benchmark 入力は wrapper struct で表し、`validate.rs` で `[benchmark.*]` がちょうど 1 つだけ指定されていることを検証する
- 旧 config 形式との互換レイヤは作らない
- `PromptConfig` の `Option` 2 本構成を廃止する
- `RunConfig` に `benchmark: BenchmarkConfig` を導入
- `src/config/*` は top-level の orchestration 層として残す
- `toml.rs` は top-level input schema と benchmark 用 wrapper struct を保持し、benchmark input schema の詳細は `benchmarks/*/config.rs` に委譲する
- `resolve.rs` は common 部分を処理し、benchmark 部分は `benchmarks/*/config.rs` に委譲する
- `validate.rs` は common validation を担当し、benchmark validation は `benchmarks/*/config.rs` に委譲する
- `metadata.rs` は `BenchmarkConfig` から既存 output schema への projection を担当し、benchmark 固有 metadata 補完は `benchmarks/*/config.rs` に委譲する

完了条件:

- 入力 config で inactive prompt section を表現できない
- 入力 config で benchmark 未指定または複数 benchmark 同時指定を validation で reject できる
- benchmark 選択と prompt id 群が同じ config section から解決される
- common validation から `prompt.*` の否定条件が消える

### Phase D: `cli` から benchmark-specific 組み立てを追い出す

目的:

- 実行の entry point を薄くする

作業:

- `load_cases` を `cli/evaluate.rs` から削除
- `build_locomo_kioku_judges` / `build_longmemeval_kioku_judges` を benchmark module 内の private helper へ移す
- benchmark ごとの `PromptBuilder` 実装を benchmark module 内へ移す
- token counter の選択も benchmark module 内へ寄せる
- 各 benchmark module に `prepare_run(...) -> PreparedBenchmarkRun<_>` を導入する
- `PreparedBenchmarkRun` に `prompt_builder` を含める
- `execute_prepared_run(...)` が bundle 内の `prompt_builder` を `run_pipeline(...)` へ渡す形にする
- protocol は prompt config を参照ではなく所有する形へ変える
- judge は prompt id を借用ではなく所有する形へ変える
- `cli/evaluate.rs` は「config を読む」「共通 backend / answerer を作る」「benchmark ごとに `prepare_run` を呼ぶ」「`execute_prepared_run(...)` で bundle を共通 pipeline へ橋渡しする」だけにする
- `run_pipeline(...)` は `PreparedBenchmarkRun` を直接受け取るようには変えず、primitive のまま維持する

完了条件:

- `match benchmark` は orchestration 層の 1 箇所にのみ残る
- `cli/evaluate.rs` から benchmark 固有の詳細実装が減り、共通 pipeline 実行の手順が読み取れる
- benchmark module の公開面に個別 builder 群ではなく `prepare_run` が立っている
- `prompt_builder` と `protocol` の組が bundle 化され、取り違え不能になっている
- `PreparedBenchmarkRun` の展開責務は thin helper に閉じ込められ、`run_pipeline(...)` の低レベル API は維持されている

### Phase E: 公開面の縮小

目的:

- `mod.rs` と `lib.rs` を本来の facade に戻す

作業:

- `lib.rs` の `pub mod` を `mod` に変更
- top-level public API を `Cli` / `run_cli` 中心へ縮小
- `model/mod.rs` の二重公開を解消
- `judge/mod.rs` / `datasets/mod.rs` / `prompt/mod.rs` の benchmark 固有 re-export を削除
- 不要になった compatibility re-export を削除

完了条件:

- `rg -n "pub mod " crates/evaluate/src` の結果が意図的な facade に限られる
- benchmark 固有型が shared module から見えなくなる

### Phase F: デッドコード削除と最終整形

目的:

- 中間互換層を除去して最終構成に固定する

作業:

- 未使用 `Judge` trait の削除
- `prompt/profiles` の廃止
- 旧 path 互換のために残した re-export の削除
- module comment と doc の更新

完了条件:

- benchmark 固有ロジックは `benchmarks/*` 配下にのみ存在する
- `mod.rs` は facade としてのみ機能する

## 9. 検証方針

各 phase 完了時に、少なくとも次を確認します。

- `cargo test -p evaluate`
- `cargo check -p evaluate`
- `rg -n "LoCoMo|LongMemEval" crates/evaluate/src/common`
- `rg -n "pub mod |pub use " crates/evaluate/src`

確認観点:

- output schema が変わっていない
- 新しい `[benchmark.<name>]` config 形式だけを config loader が受け付け、benchmark 未指定・複数指定は validation で reject する
- protocol id が変わっていない
- prompt id が変わっていない
- LoCoMo category 1-4 filtering が維持されている
- LongMemEval の abstention / context token count の扱いが維持されている

## 10. 非目的

このリファクタリングでは、次は原則として行いません。

- prompt 文面の変更
- judge rubric の意味変更
- metrics 定義の変更
- `run.resolved.json` / `answers.jsonl` / `retrieval.jsonl` / `metrics.json` の schema 変更
- 旧 `run.dataset` / `[prompt.*]` config 形式との互換維持
- backend 追加
- Phase 6 の checkpoint / resume / 並列実行の実装

まずは「どこに何があるべきか」を整理し、以後の機能追加を benchmark 単位で追える構造にすることを優先します。
