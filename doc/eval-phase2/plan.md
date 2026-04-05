# Phase 2 実装計画

## 1. 目的

Phase 2 の目的は、Phase 1 で完成した共通 runner に **LLM を使う Answerer 実装を接続し、OpenAI 互換 API で回答生成を差し替え可能にすること** です。

この段階では retrieval 指標の精密化や official judge への準拠はまだ主目的ではありません。まずは次を成立させます。

1. `LlmAnswerer` を `Answerer` の下位抽象として定義できる
2. prompt builder を `Answerer` 実装から分離できる
3. `LlmBackedAnswerer<T>` で `DebugAnswerer` と差し替え可能にできる
4. `rig-core` を使って OpenAI 互換 API を呼べる
5. CLI から `debug` と `openai-compatible` を切り替えられる

## 2. Phase 2 の完了条件

Phase 2 の完了条件は次です。

1. `LlmAnswerer` trait が定義されている
2. `LlmGenerateRequest` / `LlmGenerateResponse` が定義されている
3. prompt builder が dataset 非依存の共通部品として実装されている
4. `LlmBackedAnswerer<T>` が実装されている
5. `RigOpenAiCompatibleLlmAnswerer` が実装されている
6. CLI から `--answerer openai-compatible` を指定できる
7. model / base URL / API key 周りの設定を CLI と env var から組み立てられる
8. `DebugAnswerer` と LLM 実装の差し替えを確認できる

## 3. 前提整理

### 3.1 runner は Phase 1 で完成している前提

Phase 2 では runner の責務は増やしません。

- runner が知るのは引き続き `Answerer` まで
- LLM 呼び出しの詳細は `LlmAnswerer` 実装の内側へ閉じ込める
- prompt 構築も runner に持ち込まない

### 3.2 Phase 2 は回答生成の差し替えに集中する

この段階で解きたいのは「stub から実 LLM に置き換えられるか」です。

- retrieval 指標の精密化は Phase 3
- official judge や type-specific rubric は Phase 3 以降
- checkpoint / resume や baseline 追加は Phase 4

## 4. 実装方針

### 4.1 `Answerer` と `LlmAnswerer` を分ける

`Answerer` と `LlmAnswerer` は役割が異なります。

- `Answerer`: runner が使う高レベル I/F
- `LlmAnswerer`: LLM を使う `Answerer` 実装が内部で依存する低レベル I/F

この 2 層に分ける理由は次です。

- `DebugAnswerer` と `rig-core` 実装を同じ `Answerer` 配下で差し替えやすい
- 将来 `Answerer` を LLM 非依存に差し替える余地を残せる
- prompt 構築責務と API 呼び出し責務を分離できる

### 4.2 prompt builder を別モジュールに分ける

理由は次です。

- `LlmBackedAnswerer` と `rig-core` 実装で同じ prompt を使いたい
- dataset ごとの差分を将来調整しやすくしたい
- prompt を I/O 実装から分離したい

最小の prompt 構成は次です。

- system prompt:
  - 「与えられた memory のみを根拠に質問へ答える」
  - 「根拠が不足している場合は不足していると述べる」
- user prompt:
  - dataset 名
  - question
  - retrieved memories の列挙

few-shot はまだ不要です。

## 5. モジュール構成

Phase 2 では Phase 1 の構成に次を追加します。

```text
crates/evaluate/src/
├── answerer/
│   ├── mod.rs
│   ├── traits.rs
│   ├── debug.rs
│   ├── llm.rs
│   ├── prompt.rs
│   └── rig_openai.rs
```

Phase 1 からの差分は `answerer/llm.rs`, `answerer/prompt.rs`, `answerer/rig_openai.rs` が増えることです。

## 6. `LlmAnswerer` の I/F

```rust
#[async_trait]
pub trait LlmAnswerer {
    async fn generate(
        &self,
        request: LlmGenerateRequest<'_>,
    ) -> anyhow::Result<LlmGenerateResponse>;
}
```

`LlmGenerateRequest` に含める項目は次です。

- `system_prompt: Option<&str>`
- `user_prompt: &str`
- `temperature: Option<f32>`
- `max_output_tokens: Option<u32>`
- `metadata: &serde_json::Value`

`LlmGenerateResponse` には次を含めます。

- `text: String`
- `model_name: Option<String>`
- `raw_response: Option<serde_json::Value>`

この粒度に留める理由は、chat completion の詳細差分を抽象化し過ぎないためです。必要最小限に留めた方が `LlmBackedAnswerer` と `rig-core` 実装の両方を書きやすいです。

## 7. `Answerer` / `LlmAnswerer` の実装方針

### 7.1 `LlmBackedAnswerer`

`Answerer` 側には、`LlmAnswerer` を内包する `LlmBackedAnswerer<T>` を置きます。

責務は次です。

1. `BenchmarkQuestion` と `RetrievedMemory` 群から prompt を構築する
2. 内部の `T: LlmAnswerer` に `generate` を依頼する
3. `GeneratedAnswer` を返す

つまり、実際の差し替え点は `LlmAnswerer` 実装であり、runner は常に `Answerer` しか見ません。

### 7.2 `RigOpenAiCompatibleLlmAnswerer`

本命実装です。`rig-core` を使い、OpenAI 互換 API に接続します。

必要な設定項目は次です。

- `base_url`
- `api_key_env`
- `model`
- `temperature`
- `max_output_tokens`
- `timeout_secs`

Phase 2 では env var から受ける構成で十分です。設定ファイル対応は後回しにします。

## 8. CLI 実装計画

Phase 2 では Phase 1 の CLI を拡張します。

想定オプション:

- `--answerer debug|openai-compatible`
- `--model <name>`: `openai-compatible` のときのみ利用
- `--base-url <url>`: `openai-compatible` のときのみ利用
- `--api-key-env <env>`: 既定は `OPENAI_API_KEY`

## 9. `rig-core` 導入計画

### 9.1 追加依存

`Cargo.toml` には既に `rig-core` が workspace dependency として定義されています。Phase 2 では `crates/evaluate/Cargo.toml` 側に依存追加します。

合わせて必要になりそうなもの:

- `tokio`
- `async-trait`

### 9.2 実装ステップ

1. `RigOpenAiCompatibleConfig` を定義する
2. `LlmAnswerer for RigOpenAiCompatibleLlmAnswerer` を実装する
3. env var と CLI 引数から config を組み立てる
4. 簡単な疎通テストを追加する

### 9.3 OpenAI 互換 API 実装で吸収する差分

`LlmAnswerer` の抽象に対して、OpenAI 互換 API 実装では次を内部で吸収します。

- 認証方式
- base URL
- model 指定方法
- chat request 形式
- raw response の保持

runner や judge にこれらを漏らさないことが重要です。

## 10. 実装順序

1. `crates/evaluate/Cargo.toml` に必要依存を追加
2. prompt builder を実装
3. `LlmAnswerer` trait を定義する
4. `LlmBackedAnswerer` を実装する
5. `RigOpenAiCompatibleLlmAnswerer` を実装
6. CLI に `openai-compatible` answerer 設定を追加
7. env var と base URL 指定で呼び出せるようにする
8. `DebugAnswerer` と `rig-core` 実装の差し替えを確認する

## 11. テスト計画

Phase 2 で最低限入れるテストは次です。

1. `LlmBackedAnswerer` の prompt-to-response test
2. prompt builder の整形 test
3. `RigOpenAiCompatibleLlmAnswerer` の config 組み立て test

`rig-core` を使う実 API 呼び出しは unit test に閉じ込めず、環境変数があるときだけ走る integration test か、手動検証コマンドに寄せる方が扱いやすいです。

## 12. リスクと対策

### 12.1 `rig-core` の API 形状に I/F が合わないリスク

対策:

- `LlmAnswerer` を chat history 全体ではなく「system + user prompt」の最小抽象に留める
- `raw_response` を保持できるようにして逃げ道を作る

### 12.2 prompt 責務が runner に漏れるリスク

対策:

- prompt builder を `answerer/` 配下に分離する
- runner は `Answerer` に `AnswerRequest` を渡すだけに留める

## 13. Phase 2 完了後に着手するもの

Phase 2 の次に進める対象は次です。

1. LoCoMo の retrieval metrics
2. LongMemEval の session-level / turn-level retrieval metrics
3. LoCoMo の LLM judge
4. LongMemEval の official type-specific judge
5. `full-context` / `oracle` backend
6. `KiokuMemoryBackend`
