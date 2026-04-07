use anyhow::ensure;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::Instant;

use super::prompt::build_llm_prompt;
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
        let prompt = build_llm_prompt(request);
        let started_at = Instant::now();
        let response = self
            .llm
            .generate(LlmGenerateRequest {
                system_prompt: prompt.system_prompt.as_deref(),
                user_prompt: &prompt.user_prompt,
                temperature: self.config.temperature,
                max_output_tokens: self.config.max_output_tokens,
                metadata: &prompt.metadata,
            })
            .await?;

        let text = response.text.trim().to_string();
        ensure!(!text.is_empty(), "LLM answerer returned an empty response");

        let mut metadata = Map::new();
        metadata.insert(
            "answerer_kind".to_string(),
            Value::String(self.config.answerer_kind.to_string()),
        );
        metadata.insert(
            "retrieved_count".to_string(),
            Value::from(request.retrieved.len() as u64),
        );
        metadata.insert(
            "latency_ms".to_string(),
            Value::from(started_at.elapsed().as_millis() as u64),
        );

        if let Some(response_id) = response.response_id {
            metadata.insert("response_id".to_string(), Value::String(response_id));
        }
        if let Some(model_name) = response.model_name {
            metadata.insert("model_name".to_string(), Value::String(model_name));
        }
        if let Some(finish_reason) = response.finish_reason {
            metadata.insert("finish_reason".to_string(), Value::String(finish_reason));
        }
        if let Some(usage) = response.usage {
            metadata.insert("usage".to_string(), serde_json::to_value(usage)?);
        }
        if let Some(raw_response) = response.raw_response {
            metadata.insert("raw_response".to_string(), raw_response);
        }

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
    use crate::model::{
        AnswerRequest, BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, GoldAnswerVariant,
        RetrievedMemory,
    };
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
        assert_eq!(generated.metadata["answerer_kind"], "openai-compatible");
        assert_eq!(generated.metadata["retrieved_count"], 1);
        assert_eq!(generated.metadata["model_name"], "test-model");
        assert_eq!(generated.metadata["response_id"], "resp-1");
        assert_eq!(generated.metadata["finish_reason"], "stop");
        assert_eq!(generated.metadata["usage"]["total_tokens"], 16);
        assert!(generated.metadata["latency_ms"].as_u64().is_some());
        assert!(generated.metadata.get("model").is_none());
        assert!(generated.metadata.get("base_url").is_none());

        let requests = llm.captured_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].temperature, Some(0.2));
        assert_eq!(requests[0].max_output_tokens, Some(64));
        assert_eq!(requests[0].metadata["dataset"], "longmemeval");
        assert!(requests[0].user_prompt.contains("Retrieved Memories:"));
        assert!(requests[0].system_prompt.is_some());
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
        let case = Box::leak(Box::new(BenchmarkCase {
            dataset: BenchmarkDataset::LongMemEval,
            case_id: "longmemeval:q1".to_string(),
            events: Vec::new(),
            questions: Vec::new(),
            metadata: serde_json::Value::Null,
        }));
        let question = Box::leak(Box::new(BenchmarkQuestion {
            question_id: "longmemeval:q1:q0".to_string(),
            question: "Where does the user live now?".to_string(),
            question_timestamp: None,
            gold_answers: vec!["Kyoto".to_string()],
            evidence_event_ids: Vec::new(),
            evidence_session_ids: Vec::new(),
            category: Some(4),
            question_type: Some("multi-session".to_string()),
            gold_answer_variant: GoldAnswerVariant::Default,
            is_abstention: false,
            metadata: serde_json::Value::Null,
        }));
        let retrieved = Box::leak(Box::new(vec![RetrievedMemory {
            event_id: "event-1".to_string(),
            stream_id: "session-1".to_string(),
            timestamp: "2024-01-01T10:00:00Z".to_string(),
            content: "The user said they moved to Kyoto last month.".to_string(),
            speaker_id: Some("user".to_string()),
            speaker_name: Some("User".to_string()),
            metadata: serde_json::Value::Null,
        }]));

        AnswerRequest {
            dataset: BenchmarkDataset::LongMemEval,
            case,
            question,
            retrieved,
        }
    }
}
