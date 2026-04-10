use anyhow::{Context, anyhow, ensure};
use async_trait::async_trait;
use rig::client::CompletionClient;
use rig::completion::{self, CompletionModel};
use rig::providers::openai;
use std::future::Future;
use std::time::Duration;
use tokio::time::{sleep, timeout};

use super::llm::{LlmAnswerer, LlmGenerateRequest, LlmGenerateResponse, LlmUsage};
use crate::config::{OpenAiCompatibleAnswererConfig, OpenAiCompatibleJudgeConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigOpenAiCompatibleConfig {
    pub base_url: String,
    pub api_key_env: String,
    pub api_key: String,
    pub model: String,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
}

impl RigOpenAiCompatibleConfig {
    pub fn from_answerer_config(config: &OpenAiCompatibleAnswererConfig) -> Self {
        Self {
            base_url: config.base_url.clone(),
            api_key_env: config.api_key_env.clone(),
            api_key: std::env::var(&config.api_key_env).unwrap_or_default(),
            model: config.model.clone(),
            timeout_secs: config.timeout_secs,
            max_retries: config.max_retries,
            retry_backoff_ms: config.retry_backoff_ms,
        }
    }

    pub fn from_judge_config(config: &OpenAiCompatibleJudgeConfig) -> Self {
        Self {
            base_url: config.base_url.clone(),
            api_key_env: config.api_key_env.clone(),
            api_key: std::env::var(&config.api_key_env).unwrap_or_default(),
            model: config.model.clone(),
            timeout_secs: config.timeout_secs,
            max_retries: config.max_retries,
            retry_backoff_ms: config.retry_backoff_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RigOpenAiCompatibleLlmAnswerer {
    client: openai::CompletionsClient,
    config: RigOpenAiCompatibleConfig,
}

impl RigOpenAiCompatibleLlmAnswerer {
    pub fn new(config: RigOpenAiCompatibleConfig) -> anyhow::Result<Self> {
        let client = openai::Client::builder()
            .api_key(config.api_key.clone())
            .base_url(&config.base_url)
            .build()
            .context("failed to build rig OpenAI-compatible client")?
            .completions_api();

        Ok(Self { client, config })
    }

    pub fn from_answerer_config(config: &OpenAiCompatibleAnswererConfig) -> anyhow::Result<Self> {
        Self::new(RigOpenAiCompatibleConfig::from_answerer_config(config))
    }

    pub fn config(&self) -> &RigOpenAiCompatibleConfig {
        &self.config
    }
}

#[async_trait]
impl LlmAnswerer for RigOpenAiCompatibleLlmAnswerer {
    async fn generate(
        &self,
        request: LlmGenerateRequest<'_>,
    ) -> anyhow::Result<LlmGenerateResponse> {
        let model = self.client.completion_model(&self.config.model);
        let system_prompt = request.system_prompt.map(str::to_string);
        let user_prompt = request.user_prompt.to_string();
        let temperature = request.temperature;
        let max_output_tokens = request.max_output_tokens;

        let response = execute_with_retry(
            self.config.timeout_secs,
            self.config.max_retries,
            self.config.retry_backoff_ms,
            || {
                let model = model.clone();
                let system_prompt = system_prompt.clone();
                let user_prompt = user_prompt.clone();
                async move {
                    let mut builder = model
                        .completion_request(user_prompt)
                        .temperature_opt(temperature.map(f64::from))
                        .max_tokens_opt(max_output_tokens.map(u64::from));

                    if let Some(system_prompt) = system_prompt {
                        builder = builder.preamble(system_prompt);
                    }

                    builder
                        .send()
                        .await
                        .context("OpenAI-compatible completion request failed")
                }
            },
        )
        .await?;

        let finish_reason = response
            .raw_response
            .choices
            .first()
            .map(|choice| choice.finish_reason.clone());
        validate_response_for_evaluation(&response)?;
        let text = extract_text_response(&response);
        ensure!(
            !text.trim().is_empty(),
            "OpenAI-compatible response did not include answer text"
        );

        Ok(LlmGenerateResponse {
            text: text.trim().to_string(),
            model_name: Some(response.raw_response.model.clone()),
            response_id: Some(response.raw_response.id.clone()),
            finish_reason,
            usage: response.raw_response.usage.as_ref().map(|usage| LlmUsage {
                input_tokens: Some(usage.prompt_tokens as u64),
                output_tokens: Some((usage.total_tokens - usage.prompt_tokens) as u64),
                total_tokens: Some(usage.total_tokens as u64),
            }),
            raw_response: Some(
                serde_json::to_value(&response.raw_response)
                    .context("failed to serialize OpenAI-compatible raw response")?,
            ),
        })
    }
}

fn validate_response_for_evaluation(
    response: &completion::CompletionResponse<openai::completion::CompletionResponse>,
) -> anyhow::Result<()> {
    let choice = response
        .raw_response
        .choices
        .first()
        .context("OpenAI-compatible response did not include any choices")?;

    ensure!(
        choice.finish_reason != "content_filter",
        "OpenAI-compatible response was blocked by content filter"
    );
    ensure!(
        refusal_text(&choice.message).is_none(),
        "OpenAI-compatible response was refused by the model"
    );

    Ok(())
}

fn extract_text_response(
    response: &completion::CompletionResponse<openai::completion::CompletionResponse>,
) -> String {
    response
        .choice
        .iter()
        .filter_map(|content| match content {
            rig::completion::message::AssistantContent::Text(text) => Some(text.text().to_string()),
            rig::completion::message::AssistantContent::Reasoning(reasoning) => {
                let text = reasoning.display_text();
                (!text.is_empty()).then_some(text)
            }
            rig::completion::message::AssistantContent::ToolCall(_)
            | rig::completion::message::AssistantContent::Image(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn refusal_text(message: &openai::completion::Message) -> Option<&str> {
    let openai::completion::Message::Assistant {
        content, refusal, ..
    } = message
    else {
        return None;
    };

    refusal
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            content.iter().find_map(|item| match item {
                openai::completion::AssistantContent::Refusal { refusal } => {
                    (!refusal.trim().is_empty()).then_some(refusal.as_str())
                }
                openai::completion::AssistantContent::Text { .. } => None,
            })
        })
}

async fn execute_with_retry<F, Fut, T>(
    timeout_secs: u64,
    max_retries: u32,
    retry_backoff_ms: u64,
    mut operation: F,
) -> anyhow::Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<T>>,
{
    let mut attempt = 0u32;

    loop {
        match timeout(Duration::from_secs(timeout_secs), operation()).await {
            Ok(Ok(value)) => return Ok(value),
            Ok(Err(error)) => {
                if attempt >= max_retries {
                    return Err(error.context(format!(
                        "OpenAI-compatible request failed after {} attempt(s)",
                        attempt + 1
                    )));
                }
            }
            Err(_) => {
                if attempt >= max_retries {
                    return Err(anyhow!(
                        "OpenAI-compatible request timed out after {} second(s) across {} attempt(s)",
                        timeout_secs,
                        attempt + 1
                    ));
                }
            }
        }

        attempt += 1;
        sleep(Duration::from_millis(retry_backoff_ms)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RigOpenAiCompatibleConfig, RigOpenAiCompatibleLlmAnswerer, completion, execute_with_retry,
        openai, refusal_text, validate_response_for_evaluation,
    };
    use crate::config::OpenAiCompatibleAnswererConfig;
    use anyhow::anyhow;
    use rig::OneOrMany;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn config_builder_uses_empty_string_for_missing_api_key_env() {
        let config = RigOpenAiCompatibleConfig::from_answerer_config(&sample_answerer_config(
            "KIOKU_UNSET_OPENAI_API_KEY_TEST",
        ));

        assert_eq!(config.api_key_env, "KIOKU_UNSET_OPENAI_API_KEY_TEST");
        assert!(config.api_key.is_empty());
        assert_eq!(config.base_url, "http://localhost:11434/v1");
        assert_eq!(config.max_retries, 2);
        assert_eq!(config.retry_backoff_ms, 10);
    }

    #[test]
    fn llm_answerer_builds_rig_client_from_resolved_config() {
        let config = RigOpenAiCompatibleConfig::from_answerer_config(&sample_answerer_config(
            "KIOKU_UNSET_OPENAI_API_KEY_TEST_BUILD",
        ));
        let answerer = RigOpenAiCompatibleLlmAnswerer::new(config.clone()).unwrap();

        assert_eq!(answerer.config(), &config);
    }

    #[tokio::test]
    async fn retry_helper_retries_until_success() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let captured = attempts.clone();

        let value = execute_with_retry(1, 2, 0, move || {
            let captured = captured.clone();
            async move {
                let attempt = captured.fetch_add(1, Ordering::SeqCst);
                if attempt < 2 {
                    Err(anyhow!("temporary failure"))
                } else {
                    Ok("ok")
                }
            }
        })
        .await
        .unwrap();

        assert_eq!(value, "ok");
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_helper_fails_after_exhausting_retries() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let captured = attempts.clone();

        let error = execute_with_retry(1, 1, 0, move || {
            let captured = captured.clone();
            async move {
                captured.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(anyhow!("still failing"))
            }
        })
        .await
        .unwrap_err()
        .to_string();

        assert!(error.contains("failed after 2 attempt(s)"));
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn refusal_detector_picks_up_assistant_refusal_content() {
        let message = openai::completion::Message::Assistant {
            content: vec![openai::completion::AssistantContent::Refusal {
                refusal: "I can't comply".to_string(),
            }],
            refusal: None,
            audio: None,
            name: None,
            tool_calls: Vec::new(),
        };

        assert_eq!(refusal_text(&message), Some("I can't comply"));
    }

    #[test]
    fn evaluation_validator_rejects_content_filtered_response() {
        let response = sample_completion_response(
            "content_filter",
            openai::completion::Message::Assistant {
                content: vec![openai::completion::AssistantContent::Text {
                    text: "filtered".to_string(),
                }],
                refusal: None,
                audio: None,
                name: None,
                tool_calls: Vec::new(),
            },
            OneOrMany::one(rig::completion::message::AssistantContent::text("filtered")),
        );

        let error = validate_response_for_evaluation(&response)
            .unwrap_err()
            .to_string();
        assert!(error.contains("content filter"));
    }

    #[test]
    fn evaluation_validator_rejects_refusal_response() {
        let response = sample_completion_response(
            "stop",
            openai::completion::Message::Assistant {
                content: vec![openai::completion::AssistantContent::Refusal {
                    refusal: "I can't answer that".to_string(),
                }],
                refusal: Some("I can't answer that".to_string()),
                audio: None,
                name: None,
                tool_calls: Vec::new(),
            },
            OneOrMany::one(rig::completion::message::AssistantContent::text(
                "I can't answer that",
            )),
        );

        let error = validate_response_for_evaluation(&response)
            .unwrap_err()
            .to_string();
        assert!(error.contains("refused"));
    }

    fn sample_answerer_config(api_key_env: &str) -> OpenAiCompatibleAnswererConfig {
        OpenAiCompatibleAnswererConfig {
            base_url: "http://localhost:11434/v1".to_string(),
            model: "test-model".to_string(),
            api_key_env: api_key_env.to_string(),
            temperature: 0.2,
            max_output_tokens: 128,
            timeout_secs: 30,
            max_retries: 2,
            retry_backoff_ms: 10,
        }
    }

    fn sample_completion_response(
        finish_reason: &str,
        message: openai::completion::Message,
        choice: OneOrMany<rig::completion::message::AssistantContent>,
    ) -> completion::CompletionResponse<openai::completion::CompletionResponse> {
        completion::CompletionResponse {
            choice,
            usage: completion::Usage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                cached_input_tokens: 0,
            },
            raw_response: openai::completion::CompletionResponse {
                id: "resp-1".to_string(),
                object: "chat.completion".to_string(),
                created: 1,
                model: "test-model".to_string(),
                system_fingerprint: None,
                choices: vec![openai::completion::Choice {
                    index: 0,
                    message,
                    logprobs: None,
                    finish_reason: finish_reason.to_string(),
                }],
                usage: Some(openai::completion::Usage {
                    prompt_tokens: 10,
                    total_tokens: 15,
                    prompt_tokens_details: None,
                }),
            },
            message_id: None,
        }
    }
}
