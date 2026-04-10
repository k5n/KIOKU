use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::PromptContext;
use super::profiles::locomo;
use crate::model::{BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, RetrievedMemory};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocomoKiokuPromptConfig {
    pub answer_template_id: String,
    pub answer_judge_prompt_id: String,
    pub retrieval_judge_prompt_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongMemEvalKiokuPromptConfig {
    pub answer_template_id: String,
    pub answer_judge_prompt_id: String,
    pub retrieval_judge_prompt_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedPrompt {
    pub system_prompt: Option<String>,
    pub user_prompt: String,
    pub template_id: String,
    #[serde(default)]
    pub metadata: Value,
}

impl PreparedPrompt {
    pub fn prompt_metadata(&self) -> Value {
        let mut metadata = match self.metadata.clone() {
            Value::Object(map) => map,
            Value::Null => Map::new(),
            other => {
                let mut map = Map::new();
                map.insert("details".to_string(), other);
                map
            }
        };
        metadata.insert(
            "template_id".to_string(),
            Value::String(self.template_id.clone()),
        );
        Value::Object(metadata)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PromptBuildRequest<'a> {
    pub dataset: BenchmarkDataset,
    pub case: &'a BenchmarkCase,
    pub question: &'a BenchmarkQuestion,
    pub retrieved: &'a [RetrievedMemory],
    pub prompt_context: Option<&'a PromptContext>,
    pub locomo_kioku_prompt: Option<&'a LocomoKiokuPromptConfig>,
    pub longmemeval_kioku_prompt: Option<&'a LongMemEvalKiokuPromptConfig>,
}

pub trait PromptBuilder: Send + Sync {
    fn build_answer_prompt(
        &self,
        request: PromptBuildRequest<'_>,
    ) -> anyhow::Result<PreparedPrompt>;
}

#[derive(Debug, Clone, Default)]
pub struct DefaultPromptBuilder;

impl PromptBuilder for DefaultPromptBuilder {
    fn build_answer_prompt(
        &self,
        request: PromptBuildRequest<'_>,
    ) -> anyhow::Result<PreparedPrompt> {
        match request.dataset {
            BenchmarkDataset::LoCoMo => build_locomo_prompt(request),
            BenchmarkDataset::LongMemEval => build_longmemeval_prompt(request),
        }
    }
}

const LONGMEMEVAL_KIOKU_ANSWER_SYSTEM_PROMPT: &str = concat!(
    "You answer questions using only the provided memory context.\n",
    "Treat the provided current date as the reference time for the question.\n",
    "For knowledge-update questions, prefer the latest state supported by the memory context.\n",
    "Do not use external knowledge.\n",
    "If the memory context is insufficient, answer exactly: NOT_ENOUGH_MEMORY\n",
    "Do not explain your reasoning.\n",
    "Return only the final answer as a short phrase."
);

fn build_locomo_prompt(request: PromptBuildRequest<'_>) -> anyhow::Result<PreparedPrompt> {
    let config = request
        .locomo_kioku_prompt
        .context("LoCoMo prompt config is required to build locomo_kioku answer prompts")?;
    let resolved_context = request
        .prompt_context
        .cloned()
        .context("LoCoMo locomo_kioku answer prompt requires backend-provided prompt_context")?;

    let user_prompt = format!(
        "Memory context:\n{}\n\nQuestion:\n{}",
        resolved_context.text, request.question.question
    );

    Ok(PreparedPrompt {
        system_prompt: Some(locomo::KIOKU_ANSWER_SYSTEM_PROMPT.to_string()),
        user_prompt,
        template_id: config.answer_template_id.clone(),
        metadata: json!({
            "context_kind": resolved_context.kind,
        }),
    })
}

fn build_longmemeval_prompt(request: PromptBuildRequest<'_>) -> anyhow::Result<PreparedPrompt> {
    let config = request.longmemeval_kioku_prompt.context(
        "LongMemEval prompt config is required to build longmemeval_kioku answer prompts",
    )?;
    build_longmemeval_kioku_prompt(request, config)
}

fn build_longmemeval_kioku_prompt(
    request: PromptBuildRequest<'_>,
    config: &LongMemEvalKiokuPromptConfig,
) -> anyhow::Result<PreparedPrompt> {
    let resolved_context = request.prompt_context.cloned().context(
        "LongMemEval longmemeval_kioku answer prompt requires backend-provided prompt_context",
    )?;
    let current_date = longmemeval_question_date(request.question)?;
    let user_prompt = format!(
        "Memory context:\n{}\n\nCurrent date:\n{}\n\nQuestion:\n{}",
        resolved_context.text, current_date, request.question.question
    );

    Ok(PreparedPrompt {
        system_prompt: Some(LONGMEMEVAL_KIOKU_ANSWER_SYSTEM_PROMPT.to_string()),
        user_prompt,
        template_id: config.answer_template_id.clone(),
        metadata: json!({
            "context_kind": resolved_context.kind,
            "protocol": "longmemeval_kioku_v1",
        }),
    })
}

fn longmemeval_question_date(question: &BenchmarkQuestion) -> anyhow::Result<&str> {
    question
        .metadata
        .get("raw_question_date")
        .and_then(Value::as_str)
        .or(question.question_timestamp.as_deref())
        .context("LongMemEval question is missing prompt-ready question date metadata")
}

#[cfg(test)]
mod tests {
    use super::{
        DefaultPromptBuilder, LocomoKiokuPromptConfig, LongMemEvalKiokuPromptConfig,
        PromptBuildRequest, PromptBuilder,
    };
    use crate::model::{
        BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, GoldAnswerVariant, RetrievedMemory,
    };
    use crate::prompt::{PromptContext, PromptContextKind};

    #[test]
    fn locomo_uses_locomo_kioku_template() {
        let builder = DefaultPromptBuilder;
        let case = sample_case(BenchmarkDataset::LoCoMo);
        let question = sample_question(BenchmarkDataset::LoCoMo, Some(4));
        let context = PromptContext {
            kind: PromptContextKind::StructuredFacts,
            text: "1. [fact] The user moved to Kyoto.".to_string(),
            metadata: serde_json::Value::Null,
        };

        let prompt = builder
            .build_answer_prompt(PromptBuildRequest {
                dataset: BenchmarkDataset::LoCoMo,
                case: &case,
                question: &question,
                retrieved: &sample_retrieved(),
                prompt_context: Some(&context),
                locomo_kioku_prompt: Some(&sample_locomo_prompt_config()),
                longmemeval_kioku_prompt: None,
            })
            .unwrap();

        assert_eq!(prompt.template_id, "locomo.kioku.answer.v1");
        assert_eq!(
            prompt.system_prompt.as_deref(),
            Some(
                "You answer questions using only the provided memory context.\nDo not use external knowledge.\nIf the memory context is insufficient, answer exactly: NOT_ENOUGH_MEMORY\nReturn only the final answer as a short phrase."
            )
        );
        assert!(prompt.user_prompt.contains("Memory context:"));
    }

    #[test]
    fn locomo_rejects_missing_prompt_context() {
        let builder = DefaultPromptBuilder;
        let case = sample_case(BenchmarkDataset::LoCoMo);
        let question = sample_question(BenchmarkDataset::LoCoMo, Some(2));

        let error = builder
            .build_answer_prompt(PromptBuildRequest {
                dataset: BenchmarkDataset::LoCoMo,
                case: &case,
                question: &question,
                retrieved: &sample_retrieved(),
                prompt_context: None,
                locomo_kioku_prompt: Some(&sample_locomo_prompt_config()),
                longmemeval_kioku_prompt: None,
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("requires backend-provided prompt_context"));
    }

    #[test]
    fn longmemeval_kioku_uses_fixed_template_and_system_prompt() {
        let builder = DefaultPromptBuilder;
        let case = sample_case(BenchmarkDataset::LongMemEval);
        let question = sample_question(BenchmarkDataset::LongMemEval, None);
        let context = PromptContext {
            kind: PromptContextKind::HistoryChats,
            text: "### Session 1\nSession Content:\nuser: moved to Kyoto".to_string(),
            metadata: serde_json::Value::Null,
        };

        let prompt = builder
            .build_answer_prompt(PromptBuildRequest {
                dataset: BenchmarkDataset::LongMemEval,
                case: &case,
                question: &question,
                retrieved: &sample_retrieved(),
                prompt_context: Some(&context),
                locomo_kioku_prompt: None,
                longmemeval_kioku_prompt: Some(&sample_longmemeval_kioku_prompt_config()),
            })
            .unwrap();

        assert_eq!(prompt.template_id, "longmemeval.kioku.answer.v1");
        assert_eq!(
            prompt.system_prompt.as_deref(),
            Some(super::LONGMEMEVAL_KIOKU_ANSWER_SYSTEM_PROMPT)
        );
        assert!(prompt.user_prompt.contains("Memory context:"));
        assert!(prompt.user_prompt.contains("Current date:\n2024-01-03"));
    }

    #[test]
    fn longmemeval_kioku_rejects_missing_prompt_context() {
        let builder = DefaultPromptBuilder;
        let case = sample_case(BenchmarkDataset::LongMemEval);
        let question = sample_question(BenchmarkDataset::LongMemEval, None);

        let error = builder
            .build_answer_prompt(PromptBuildRequest {
                dataset: BenchmarkDataset::LongMemEval,
                case: &case,
                question: &question,
                retrieved: &sample_retrieved(),
                prompt_context: None,
                locomo_kioku_prompt: None,
                longmemeval_kioku_prompt: Some(&sample_longmemeval_kioku_prompt_config()),
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("requires backend-provided prompt_context"));
    }

    #[test]
    fn longmemeval_rejects_missing_kioku_prompt_config() {
        let builder = DefaultPromptBuilder;
        let case = sample_case(BenchmarkDataset::LongMemEval);
        let question = sample_question(BenchmarkDataset::LongMemEval, None);

        let error = builder
            .build_answer_prompt(PromptBuildRequest {
                dataset: BenchmarkDataset::LongMemEval,
                case: &case,
                question: &question,
                retrieved: &sample_retrieved(),
                prompt_context: None,
                locomo_kioku_prompt: None,
                longmemeval_kioku_prompt: None,
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("LongMemEval prompt config is required"));
    }

    fn sample_case(dataset: BenchmarkDataset) -> BenchmarkCase {
        BenchmarkCase {
            dataset,
            case_id: format!("{}:case-1", dataset.as_str()),
            events: Vec::new(),
            questions: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    fn sample_question(dataset: BenchmarkDataset, category: Option<u8>) -> BenchmarkQuestion {
        BenchmarkQuestion {
            question_id: format!("{}:case-1:q0", dataset.as_str()),
            question: "Where does the user live now?".to_string(),
            question_timestamp: Some("2024-01-03T00:00:00+00:00".to_string()),
            gold_answers: vec!["Kyoto".to_string()],
            evidence_event_ids: Vec::new(),
            evidence_session_ids: Vec::new(),
            category,
            question_type: Some("multi-session".to_string()),
            gold_answer_variant: GoldAnswerVariant::Default,
            is_abstention: false,
            metadata: serde_json::json!({
                "raw_question_date": "2024-01-03",
            }),
        }
    }

    fn sample_retrieved() -> Vec<RetrievedMemory> {
        vec![RetrievedMemory {
            memory_id: "event-1".to_string(),
            source_event_id: Some("event-1".to_string()),
            source_session_id: Some("session-1".to_string()),
            score: None,
            timestamp: Some("2024-01-01T10:00:00Z".to_string()),
            content: "The user moved to Kyoto.".to_string(),
            metadata: serde_json::json!({
                "speaker_id": "user",
                "speaker_name": "User",
            }),
        }]
    }

    fn sample_locomo_prompt_config() -> LocomoKiokuPromptConfig {
        LocomoKiokuPromptConfig {
            answer_template_id: "locomo.kioku.answer.v1".to_string(),
            answer_judge_prompt_id: "locomo.kioku.judge.answer.v1".to_string(),
            retrieval_judge_prompt_id: "locomo.kioku.judge.retrieval.v1".to_string(),
        }
    }

    fn sample_longmemeval_kioku_prompt_config() -> LongMemEvalKiokuPromptConfig {
        LongMemEvalKiokuPromptConfig {
            answer_template_id: "longmemeval.kioku.answer.v1".to_string(),
            answer_judge_prompt_id: "longmemeval.kioku.judge.answer.v1".to_string(),
            retrieval_judge_prompt_id: "longmemeval.kioku.judge.retrieval.v1".to_string(),
        }
    }
}
