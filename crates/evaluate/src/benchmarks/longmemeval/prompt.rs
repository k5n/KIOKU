use anyhow::Context;
use serde_json::{Map, Value, json};

use crate::common::prompt::{PreparedPrompt, PromptBuildRequest, PromptBuilder, PromptContext};

use super::config::LongMemEvalKiokuPromptConfig;

const ANSWER_SYSTEM_PROMPT: &str = concat!(
    "You answer questions using only the provided memory prompt.\n",
    "The memory prompt may use any textual format chosen by the memory system.\n",
    "Treat the provided current date as the reference time for the question.\n",
    "For knowledge-update questions, prefer the latest state supported by the memory prompt.\n",
    "Do not use external knowledge.\n",
    "If the memory prompt is insufficient, answer exactly: NOT_ENOUGH_MEMORY\n",
    "Do not explain your reasoning.\n",
    "Return only the final answer as a short phrase."
);

#[derive(Debug, Clone)]
pub(crate) struct LongMemEvalPromptBuilder {
    config: LongMemEvalKiokuPromptConfig,
}

impl LongMemEvalPromptBuilder {
    pub(crate) fn new(config: LongMemEvalKiokuPromptConfig) -> Self {
        Self { config }
    }
}

impl PromptBuilder for LongMemEvalPromptBuilder {
    fn build_answer_prompt(
        &self,
        request: PromptBuildRequest<'_>,
    ) -> anyhow::Result<PreparedPrompt> {
        let current_date = longmemeval_question_date(request.question)?;
        let user_prompt = format!(
            "Memory prompt:\n{}\n\nCurrent date:\n{}\n\nQuestion:\n{}",
            request.prompt_context.text, current_date, request.question.question
        );

        Ok(PreparedPrompt {
            system_prompt: Some(ANSWER_SYSTEM_PROMPT.to_string()),
            user_prompt,
            template_id: self.config.answer_template_id.clone(),
            metadata: prompt_metadata(
                request.prompt_context,
                json!({
                    "protocol": "longmemeval_kioku_v1",
                }),
            ),
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

fn longmemeval_question_date(
    question: &crate::common::model::BenchmarkQuestion,
) -> anyhow::Result<&str> {
    question
        .metadata
        .get("raw_question_date")
        .and_then(Value::as_str)
        .or(question.question_timestamp.as_deref())
        .context("LongMemEval question is missing prompt-ready question date metadata")
}

#[cfg(test)]
mod tests {
    use super::{LongMemEvalKiokuPromptConfig, LongMemEvalPromptBuilder};
    use crate::common::{
        model::{BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, GoldAnswerVariant},
        prompt::{PromptBuildRequest, PromptBuilder, PromptContext, PromptContextKind},
    };

    #[test]
    fn longmemeval_kioku_uses_fixed_template_and_system_prompt() {
        let builder = LongMemEvalPromptBuilder::new(sample_prompt_config());
        let case = sample_case();
        let question = sample_question();
        let context = PromptContext {
            kind: PromptContextKind::MemoryPrompt,
            text: "### Session 1\nSession Content:\nuser: moved to Kyoto".to_string(),
            metadata: serde_json::Value::Null,
        };

        let prompt = builder
            .build_answer_prompt(PromptBuildRequest {
                case: &case,
                question: &question,
                prompt_context: &context,
            })
            .unwrap();

        assert_eq!(prompt.template_id, "longmemeval.kioku.answer.v1");
        assert_eq!(
            prompt.system_prompt.as_deref(),
            Some(super::ANSWER_SYSTEM_PROMPT)
        );
        assert!(prompt.user_prompt.contains("Memory prompt:"));
        assert!(prompt.user_prompt.contains("Current date:\n2024-01-03"));
        assert_eq!(prompt.metadata["context_kind"], "memory-prompt");
    }

    fn sample_case() -> BenchmarkCase {
        BenchmarkCase {
            dataset: BenchmarkDataset::LongMemEval,
            case_id: "longmemeval:case-1".to_string(),
            events: Vec::new(),
            questions: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    fn sample_question() -> BenchmarkQuestion {
        BenchmarkQuestion {
            question_id: "longmemeval:case-1:q0".to_string(),
            question: "Where does the user live now?".to_string(),
            question_timestamp: Some("2024-01-03T00:00:00+00:00".to_string()),
            gold_answers: vec!["Kyoto".to_string()],
            evidence_event_ids: Vec::new(),
            evidence_session_ids: Vec::new(),
            category: None,
            question_type: Some("multi-session".to_string()),
            gold_answer_variant: GoldAnswerVariant::Default,
            is_abstention: false,
            metadata: serde_json::json!({
                "raw_question_date": "2024-01-03",
            }),
        }
    }

    fn sample_prompt_config() -> LongMemEvalKiokuPromptConfig {
        LongMemEvalKiokuPromptConfig {
            answer_template_id: "longmemeval.kioku.answer.v1".to_string(),
            answer_judge_prompt_id: "longmemeval.kioku.judge.answer.v1".to_string(),
            retrieval_judge_prompt_id: "longmemeval.kioku.judge.retrieval.v1".to_string(),
        }
    }
}
