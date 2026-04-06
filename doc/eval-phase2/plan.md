# Phase 2 実装計画

## 1. 目的

Phase 2 の目的は、Phase 1.5 で TOML ベースの設定実行へ移行した共通 runner に **LLM を使う Answerer 実装を接続し、OpenAI 互換 API で回答生成を差し替え可能にすること** です。

この段階では retrieval 指標の精密化や official judge への準拠はまだ主目的ではありません。まずは次を成立させます。

1. `LlmAnswerer` を `Answerer` の下位抽象として定義できる
2. prompt builder を `Answerer` 実装から分離できる
3. `LlmBackedAnswerer<T>` で `DebugAnswerer` と差し替え可能にできる
4. `rig-core` を使って OpenAI 互換 API を呼べる
5. TOML 設定ファイルから `debug` と `openai-compatible` を切り替えられる

## 2. Phase 2 の完了条件

Phase 2 の完了条件は次です。

1. `LlmAnswerer` trait が定義されている
2. `LlmGenerateRequest` / `LlmGenerateResponse` が定義されている
3. prompt builder が共通 core と dataset 別の拡張点を持つ形で実装されている
4. `LlmBackedAnswerer<T>` が実装されている
5. `RigOpenAiCompatibleLlmAnswerer` が実装されている
6. TOML 設定で `answerer.kind = "openai-compatible"` を指定できる
7. model / base URL / API key 周りの設定を TOML と env var から組み立てられる
8. `DebugAnswerer` と LLM 実装の差し替えを確認できる
9. `answer_metadata` に解決済み実験設定を残せる

## 3. 前提整理

### 3.1 runner は Phase 1 で完成し、Phase 1.5 で設定基盤へ移行済み

Phase 2 では runner の責務は増やしません。

- runner が知るのは引き続き `Answerer` まで
- LLM 呼び出しの詳細は `LlmAnswerer` 実装の内側へ閉じ込める
- prompt 構築も runner に持ち込まない
- 実行設定は Phase 1.5 で導入した TOML 設定構造体から受け取る

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

Phase 2 の prompt builder は、完全に dataset 非依存な 1 本に固定せず、**共通 core と dataset 別の拡張点** を持つ形にします。

最小の prompt 構成は次です。

- system prompt:
  - 「与えられた memory のみを根拠に質問へ答える」
  - 「根拠が不足している場合は不足していると述べる」
- user prompt:
  - dataset 名
  - question
  - retrieved memories の列挙

few-shot はまだ不要です。

ただし、将来の差分吸収のため次は参照できるようにします。

- `dataset`
- `question_type`
- `is_abstention`
- `category`

## 5. モジュール構成

Phase 2 では Phase 1.5 の構成に次を追加します。

```text
crates/evaluate/src/
├── config/
│   ├── mod.rs
│   └── run.rs
├── answerer/
│   ├── mod.rs
│   ├── traits.rs
│   ├── debug.rs
│   ├── llm.rs
│   ├── prompt.rs
│   └── rig_openai.rs
```

Phase 1.5 からの差分は `answerer/llm.rs`, `answerer/prompt.rs`, `answerer/rig_openai.rs` が増えることです。

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
- `finish_reason: Option<String>`
- `raw_response: Option<serde_json::Value>`

この粒度に留める理由は、chat completion の詳細差分を抽象化し過ぎないためです。必要最小限に留めた方が `LlmBackedAnswerer` と `rig-core` 実装の両方を書きやすいです。

また、空応答や content filter などで採点に回すべきでない失敗を識別できるよう、`LlmAnswerer` 実装は「有効な回答テキストが得られなかった場合に `Err` を返す」方針を採ります。

## 7. `Answerer` / `LlmAnswerer` の実装方針

### 7.1 `LlmBackedAnswerer`

`Answerer` 側には、`LlmAnswerer` を内包する `LlmBackedAnswerer<T>` を置きます。

責務は次です。

1. `BenchmarkQuestion` と `RetrievedMemory` 群から prompt を構築する
2. 内部の `T: LlmAnswerer` に `generate` を依頼する
3. 解決済み実験設定と LLM 応答 metadata を `GeneratedAnswer.metadata` に詰めて返す

`GeneratedAnswer.metadata` には少なくとも次を残します。

- `answerer_kind`
- `model`
- `base_url`
- `temperature`
- `max_output_tokens`
- `timeout_secs`
- `api_key_env`
- `config_path`
- `retrieved_count`

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

これらは Phase 1.5 で導入した TOML 設定ファイルから受け取り、`api_key_env` は optional とします。

- `api_key_env = Some(...)` のときだけ env var を解決する
- `api_key_env = None` のときは Authorization ヘッダなしで呼ぶ

これにより OpenAI 系 API と、llama.cpp / Ollama のようなローカル OpenAI 互換 API を同じ I/F で扱えます。

## 8. 設定ファイル実装計画

Phase 2 では CLI 自体は Phase 1.5 の `--config <path>` のままとし、TOML の設定モデルだけを拡張します。

想定する answerer 設定項目:

- `answerer.kind = "debug" | "openai-compatible"`
- `answerer.openai_compatible.base_url`
- `answerer.openai_compatible.model`
- `answerer.openai_compatible.api_key_env`
- `answerer.openai_compatible.temperature`
- `answerer.openai_compatible.max_output_tokens`
- `answerer.openai_compatible.timeout_secs`

## 9. `rig-core` 導入計画

### 9.1 追加依存

`Cargo.toml` には既に `rig-core` が workspace dependency として定義されています。Phase 2 では `crates/evaluate/Cargo.toml` 側に依存追加します。

Phase 1 時点で `tokio` と `async-trait` は導入済みのため、Phase 2 で主に追加するのは `rig-core` と TOML 設定の補助依存です。

### 9.2 実装ステップ

1. `RigOpenAiCompatibleConfig` を定義する
2. `LlmAnswerer for RigOpenAiCompatibleLlmAnswerer` を実装する
3. TOML 設定と env var から config を組み立てる
4. `api_key_env = None` の分岐を実装する
5. 簡単な疎通テストを追加する

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
2. TOML 設定型に `openai-compatible` answerer 設定を追加
3. prompt builder を実装
4. `LlmAnswerer` trait を定義する
5. `LlmBackedAnswerer` を実装する
6. `RigOpenAiCompatibleLlmAnswerer` を実装
7. TOML 設定と env var から config を解決できるようにする
8. `GeneratedAnswer.metadata` に解決済み実験設定を残す
9. `DebugAnswerer` と `rig-core` 実装の差し替えを確認する

## 11. テスト計画

Phase 2 で最低限入れるテストは次です。

1. `LlmBackedAnswerer` の prompt-to-response test
2. prompt builder の整形 test
3. `RigOpenAiCompatibleLlmAnswerer` の config 組み立て test
4. `api_key_env = None` を許容する test
5. 空応答を error 扱いにする test
6. `answer_metadata` に解決済み LLM 設定が載る test

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

### 12.3 dataset 差分を無視した prompt になってしまうリスク

対策:

- prompt builder は共通 core と dataset 別の拡張点を持つ形にする
- `dataset`, `question_type`, `is_abstention`, `category` を参照できるようにする

### 12.4 LLM 呼び出し失敗を不正解として集計してしまうリスク

対策:

- 空応答や無効応答は `Err` として扱う
- `answer_metadata` に finish reason や raw response を残せるようにする

## 13. Phase 2 完了後に着手するもの

Phase 2 の次に進める対象は次です。

1. LoCoMo の retrieval metrics
2. LongMemEval の session-level / turn-level retrieval metrics
3. LoCoMo の LLM judge
4. LongMemEval の official type-specific judge
5. `full-context` / `oracle` backend
6. `KiokuMemoryBackend`
