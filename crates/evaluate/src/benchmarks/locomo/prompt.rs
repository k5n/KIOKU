use serde_json::{Map, Value, json};

use crate::common::prompt::{PreparedPrompt, PromptBuildRequest, PromptBuilder, PromptContext};

use super::config::LocomoKiokuPromptConfig;

const KIOKU_ANSWER_SYSTEM_PROMPT: &str = concat!(
    "You answer questions using only the provided memory prompt.\n",
    "The memory prompt may use any textual format chosen by the memory system.\n",
    "Do not use external knowledge.\n",
    "If the memory prompt is insufficient, answer exactly: NOT_ENOUGH_MEMORY\n",
    "Return only the final answer as a short phrase."
);

#[derive(Debug, Clone)]
pub(crate) struct LocomoPromptBuilder {
    config: LocomoKiokuPromptConfig,
}

impl LocomoPromptBuilder {
    pub(crate) fn new(config: LocomoKiokuPromptConfig) -> Self {
        Self { config }
    }
}

impl PromptBuilder for LocomoPromptBuilder {
    fn build_answer_prompt(
        &self,
        request: PromptBuildRequest<'_>,
    ) -> anyhow::Result<PreparedPrompt> {
        let user_prompt = format!(
            "Memory prompt:\n{}\n\nQuestion:\n{}",
            request.prompt_context.text, request.question.question
        );

        Ok(PreparedPrompt {
            system_prompt: Some(KIOKU_ANSWER_SYSTEM_PROMPT.to_string()),
            user_prompt,
            template_id: self.config.answer_template_id.clone(),
            metadata: prompt_metadata(request.prompt_context, json!({})),
        })
    }
}

fn prompt_metadata(context: &PromptContext, metadata: Value) -> Value {
    let mut merged = match metadata {
        Value::Object(map) => map,
        Value::Null => Map::new(),
        other => {
            let mut map = Map::new();
            map.insert("details".to_string(), other);
            map
        }
    };
    merged.insert("context_kind".to_string(), json!(context.kind));
    if let Value::Object(context_metadata) = &context.metadata {
        merged.extend(context_metadata.clone());
    }
    Value::Object(merged)
}

#[cfg(test)]
mod tests {
    use super::{LocomoKiokuPromptConfig, LocomoPromptBuilder};
    use crate::common::{
        model::{BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, GoldAnswerVariant},
        prompt::{PromptBuildRequest, PromptBuilder, PromptContext, PromptContextKind},
    };

    #[test]
    fn locomo_uses_locomo_kioku_template() {
        let builder = LocomoPromptBuilder::new(sample_prompt_config());
        let case = sample_case();
        let question = sample_question();
        let context = PromptContext {
            kind: PromptContextKind::MemoryPrompt,
            text: "1. [fact] The user moved to Kyoto.".to_string(),
            metadata: serde_json::Value::Null,
        };

        let prompt = builder
            .build_answer_prompt(PromptBuildRequest {
                case: &case,
                question: &question,
                prompt_context: &context,
            })
            .unwrap();

        assert_eq!(prompt.template_id, "locomo.kioku.answer.v1");
        assert_eq!(
            prompt.system_prompt.as_deref(),
            Some(super::KIOKU_ANSWER_SYSTEM_PROMPT)
        );
        assert!(prompt.user_prompt.contains("Memory prompt:"));
        assert_eq!(prompt.metadata["context_kind"], "memory-prompt");
    }

    fn sample_case() -> BenchmarkCase {
        BenchmarkCase {
            dataset: BenchmarkDataset::LoCoMo,
            case_id: "locomo:case-1".to_string(),
            events: Vec::new(),
            questions: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    fn sample_question() -> BenchmarkQuestion {
        BenchmarkQuestion {
            question_id: "locomo:case-1:q0".to_string(),
            question: "Where does the user live now?".to_string(),
            question_timestamp: Some("2024-01-03T00:00:00+00:00".to_string()),
            gold_answers: vec!["Kyoto".to_string()],
            evidence_event_ids: Vec::new(),
            evidence_session_ids: Vec::new(),
            category: Some(4),
            question_type: None,
            gold_answer_variant: GoldAnswerVariant::Default,
            is_abstention: false,
            metadata: serde_json::Value::Null,
        }
    }

    fn sample_prompt_config() -> LocomoKiokuPromptConfig {
        LocomoKiokuPromptConfig {
            answer_template_id: "locomo.kioku.answer.v1".to_string(),
            answer_judge_prompt_id: "locomo.kioku.judge.answer.v1".to_string(),
            retrieval_judge_prompt_id: "locomo.kioku.judge.retrieval.v1".to_string(),
        }
    }
}
