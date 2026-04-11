use async_trait::async_trait;
use serde_json::{Map, Value};

use crate::answerer::Answerer;
use crate::model::{AnswerRequest, GeneratedAnswer};

#[derive(Debug, Clone)]
pub struct DebugAnswerer {
    fixed_answer: String,
}

impl Default for DebugAnswerer {
    fn default() -> Self {
        Self {
            fixed_answer: "[debug-answer]".to_string(),
        }
    }
}

impl DebugAnswerer {
    pub fn new(fixed_answer: impl Into<String>) -> Self {
        Self {
            fixed_answer: fixed_answer.into(),
        }
    }
}

#[async_trait]
impl Answerer for DebugAnswerer {
    async fn answer(&self, request: AnswerRequest<'_>) -> anyhow::Result<GeneratedAnswer> {
        let mut metadata = Map::new();
        metadata.insert("prompt".to_string(), request.prompt.prompt_metadata());
        metadata.insert(
            "answerer".to_string(),
            serde_json::json!({
                "kind": "debug",
                "mode": "fixed",
            }),
        );

        Ok(GeneratedAnswer {
            text: self.fixed_answer.clone(),
            metadata: Value::Object(metadata),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DebugAnswerer;
    use crate::answerer::Answerer;
    use crate::model::AnswerRequest;
    use crate::prompt::PreparedPrompt;

    #[tokio::test]
    async fn returns_fixed_answer_with_debug_metadata() {
        let answerer = DebugAnswerer::default();
        let prompt = PreparedPrompt {
            system_prompt: None,
            user_prompt: "Question".to_string(),
            template_id: "locomo.qa.default.v1".to_string(),
            metadata: serde_json::json!({
                "requested_profile": serde_json::Value::Null,
                "resolved_profile": serde_json::Value::Null,
                "context_kind": "memory-prompt",
            }),
        };

        let generated = answerer
            .answer(AnswerRequest { prompt: &prompt })
            .await
            .unwrap();

        assert_eq!(generated.text, "[debug-answer]");
        assert_eq!(generated.metadata["answerer"]["kind"], "debug");
        assert_eq!(
            generated.metadata["prompt"]["template_id"],
            "locomo.qa.default.v1"
        );
    }
}
