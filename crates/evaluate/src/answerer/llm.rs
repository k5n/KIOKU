use anyhow::ensure;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::Instant;

use crate::answerer::Answerer;
use crate::model::{AnswerRequest, GeneratedAnswer};

#[derive(Debug, Clone, PartialEq)]
pub struct LlmGenerateRequest<'a> {
    pub system_prompt: Option<&'a str>,
    pub user_prompt: &'a str,
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
    pub metadata: &'a Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LlmGenerateResponse {
    pub text: String,
    pub model_name: Option<String>,
    pub response_id: Option<String>,
    pub finish_reason: Option<String>,
    pub usage: Option<LlmUsage>,
    pub raw_response: Option<Value>,
}

#[async_trait]
pub trait LlmAnswerer: Send + Sync {
    async fn generate(
        &self,
        request: LlmGenerateRequest<'_>,
    ) -> anyhow::Result<LlmGenerateResponse>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmBackedAnswererConfig {
    pub answerer_kind: &'static str,
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LlmBackedAnswerer<T> {
    config: LlmBackedAnswererConfig,
    llm: T,
}

impl<T> LlmBackedAnswerer<T> {
    pub fn new(config: LlmBackedAnswererConfig, llm: T) -> Self {
        Self { config, llm }
    }
}

#[async_trait]
impl<T> Answerer for LlmBackedAnswerer<T>
where
    T: LlmAnswerer,
{
    async fn answer(&self, request: AnswerRequest<'_>) -> anyhow::Result<GeneratedAnswer> {
        let started_at = Instant::now();
        let response = self
            .llm
            .generate(LlmGenerateRequest {
                system_prompt: request.prompt.system_prompt.as_deref(),
                user_prompt: &request.prompt.user_prompt,
                temperature: self.config.temperature,
                max_output_tokens: self.config.max_output_tokens,
                metadata: &request.prompt.metadata,
            })
            .await?;

        let text = response.text.trim().to_string();
        ensure!(!text.is_empty(), "LLM answerer returned an empty response");

        let mut metadata = Map::new();
        metadata.insert("prompt".to_string(), request.prompt.prompt_metadata());
        metadata.insert(
            "answerer".to_string(),
            serde_json::json!({
                "kind": self.config.answerer_kind,
            }),
        );
        let mut llm_metadata = Map::new();
        llm_metadata.insert(
            "latency_ms".to_string(),
            Value::from(started_at.elapsed().as_millis() as u64),
        );

        if let Some(response_id) = response.response_id {
            llm_metadata.insert("response_id".to_string(), Value::String(response_id));
        }
        if let Some(model_name) = response.model_name {
            llm_metadata.insert("model_name".to_string(), Value::String(model_name));
        }
        if let Some(finish_reason) = response.finish_reason {
            llm_metadata.insert("finish_reason".to_string(), Value::String(finish_reason));
        }
        if let Some(usage) = response.usage {
            llm_metadata.insert("usage".to_string(), serde_json::to_value(usage)?);
        }
        if let Some(raw_response) = response.raw_response {
            llm_metadata.insert("raw_response".to_string(), raw_response);
        }
        metadata.insert("llm".to_string(), Value::Object(llm_metadata));

        Ok(GeneratedAnswer {
            text,
            metadata: Value::Object(metadata),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LlmAnswerer, LlmBackedAnswerer, LlmBackedAnswererConfig, LlmGenerateRequest,
        LlmGenerateResponse, LlmUsage,
    };
    use crate::answerer::Answerer;
    use crate::model::AnswerRequest;
    use crate::prompt::PreparedPrompt;
    use anyhow::{Result, anyhow};
    use async_trait::async_trait;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone, PartialEq)]
    struct CapturedRequest {
        system_prompt: Option<String>,
        user_prompt: String,
        temperature: Option<f32>,
        max_output_tokens: Option<u32>,
        metadata: serde_json::Value,
    }

    #[derive(Debug, Clone, Default)]
    struct FakeLlm {
        requests: Arc<Mutex<Vec<CapturedRequest>>>,
        responses: Arc<Mutex<VecDeque<Result<LlmGenerateResponse>>>>,
    }

    impl FakeLlm {
        fn with_responses(responses: Vec<Result<LlmGenerateResponse>>) -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(VecDeque::from(responses))),
            }
        }

        fn captured_requests(&self) -> Vec<CapturedRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl LlmAnswerer for FakeLlm {
        async fn generate(
            &self,
            request: LlmGenerateRequest<'_>,
        ) -> anyhow::Result<LlmGenerateResponse> {
            self.requests.lock().unwrap().push(CapturedRequest {
                system_prompt: request.system_prompt.map(str::to_string),
                user_prompt: request.user_prompt.to_string(),
                temperature: request.temperature,
                max_output_tokens: request.max_output_tokens,
                metadata: request.metadata.clone(),
            });

            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(anyhow!("missing fake LLM response")))
        }
    }

    #[tokio::test]
    async fn llm_backed_answerer_builds_prompt_and_records_per_response_metadata() {
        let llm = FakeLlm::with_responses(vec![Ok(LlmGenerateResponse {
            text: "Kyoto".to_string(),
            model_name: Some("test-model".to_string()),
            response_id: Some("resp-1".to_string()),
            finish_reason: Some("stop".to_string()),
            usage: Some(LlmUsage {
                input_tokens: Some(12),
                output_tokens: Some(4),
                total_tokens: Some(16),
            }),
            raw_response: Some(serde_json::json!({"id": "resp-1"})),
        })]);
        let answerer = LlmBackedAnswerer::new(
            LlmBackedAnswererConfig {
                answerer_kind: "openai-compatible",
                temperature: Some(0.2),
                max_output_tokens: Some(64),
            },
            llm.clone(),
        );
        let request = sample_request();

        let generated = answerer.answer(request).await.unwrap();

        assert_eq!(generated.text, "Kyoto");
        assert_eq!(generated.metadata["answerer"]["kind"], "openai-compatible");
        assert_eq!(generated.metadata["llm"]["model_name"], "test-model");
        assert_eq!(generated.metadata["llm"]["response_id"], "resp-1");
        assert_eq!(generated.metadata["llm"]["finish_reason"], "stop");
        assert_eq!(generated.metadata["llm"]["usage"]["total_tokens"], 16);
        assert!(generated.metadata["llm"]["latency_ms"].as_u64().is_some());
        assert_eq!(
            generated.metadata["prompt"]["template_id"],
            "longmemeval.answer.history_chats.v1"
        );

        let requests = llm.captured_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].temperature, Some(0.2));
        assert_eq!(requests[0].max_output_tokens, Some(64));
        assert_eq!(requests[0].metadata["requested_profile"], "history-chats");
        assert!(requests[0].user_prompt.contains("History Chats:"));
        assert!(requests[0].system_prompt.is_none());
    }

    #[tokio::test]
    async fn llm_backed_answerer_rejects_empty_text() {
        let answerer = LlmBackedAnswerer::new(
            LlmBackedAnswererConfig {
                answerer_kind: "openai-compatible",
                temperature: None,
                max_output_tokens: None,
            },
            FakeLlm::with_responses(vec![Ok(LlmGenerateResponse {
                text: "   ".to_string(),
                model_name: None,
                response_id: None,
                finish_reason: None,
                usage: None,
                raw_response: None,
            })]),
        );

        let error = answerer
            .answer(sample_request())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("empty response"));
    }

    fn sample_request() -> AnswerRequest<'static> {
        let prompt = Box::leak(Box::new(PreparedPrompt {
            system_prompt: None,
            user_prompt: concat!(
                "I will give you several history chats between you and a user. ",
                "Please answer the question based on the relevant chat history.\n\n\n",
                "History Chats:\n\n### Session 1:\nSession Date: 2024-01-01\n",
                "Session Content:\nuser: The user said they moved to Kyoto last month.\n\n",
                "Current Date: 2024-01-03\nQuestion: Where does the user live now?\nAnswer:"
            )
            .to_string(),
            template_id: "longmemeval.answer.history_chats.v1".to_string(),
            metadata: serde_json::json!({
                "requested_profile": "history-chats",
                "resolved_profile": "history-chats",
                "context_kind": "HistoryChats",
            }),
        }));

        AnswerRequest { prompt }
    }
}
