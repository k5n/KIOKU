use anyhow::{Context, ensure};
use serde_json::Value;

use crate::answerer::{LlmAnswerer, LlmGenerateRequest, LlmGenerateResponse};

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleJudgeRuntime<T> {
    llm: T,
    model_name: String,
    temperature: Option<f32>,
    max_output_tokens: Option<u32>,
}

impl<T> OpenAiCompatibleJudgeRuntime<T> {
    pub fn new(
        llm: T,
        model_name: impl Into<String>,
        temperature: Option<f32>,
        max_output_tokens: Option<u32>,
    ) -> Self {
        Self {
            llm,
            model_name: model_name.into(),
            temperature,
            max_output_tokens,
        }
    }
}

impl<T> OpenAiCompatibleJudgeRuntime<T>
where
    T: LlmAnswerer,
{
    pub async fn generate_json(
        &self,
        judge_kind: &str,
        prompt_id: &str,
        system_prompt: &str,
        user_prompt: String,
    ) -> anyhow::Result<(Value, LlmGenerateResponse)> {
        let response = self
            .llm
            .generate(LlmGenerateRequest {
                system_prompt: Some(system_prompt),
                user_prompt: &user_prompt,
                temperature: self.temperature,
                max_output_tokens: self.max_output_tokens,
                metadata: &serde_json::json!({
                    "judge_kind": judge_kind,
                    "judge_prompt_id": prompt_id,
                }),
            })
            .await?;
        let payload: Value = serde_json::from_str(response.text.trim()).with_context(|| {
            format!(
                "{judge_kind} expected JSON-only output for prompt `{prompt_id}`, got `{}`",
                response.text.trim()
            )
        })?;
        ensure!(
            payload.is_object(),
            "{judge_kind} expected a JSON object for prompt `{prompt_id}`"
        );
        Ok((payload, response))
    }

    pub fn resolved_model_name(&self, response: &LlmGenerateResponse) -> String {
        response
            .model_name
            .clone()
            .unwrap_or_else(|| self.model_name.clone())
    }
}
