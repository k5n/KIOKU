use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::profiles::{locomo, longmemeval};
use super::{PromptContext, PromptContextKind};
use crate::model::{BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, RetrievedMemory};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LongMemEvalAnswerPromptProfile {
    NoRetrieval,
    HistoryChats,
    HistoryChatsWithFacts,
    FactsOnly,
}

impl LongMemEvalAnswerPromptProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoRetrieval => "no-retrieval",
            Self::HistoryChats => "history-chats",
            Self::HistoryChatsWithFacts => "history-chats-with-facts",
            Self::FactsOnly => "facts-only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongMemEvalPromptConfig {
    pub answer_profile: LongMemEvalAnswerPromptProfile,
    pub cot: bool,
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
    pub longmemeval_prompt: Option<LongMemEvalPromptConfig>,
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

fn build_locomo_prompt(request: PromptBuildRequest<'_>) -> anyhow::Result<PreparedPrompt> {
    let resolved_context = request
        .prompt_context
        .cloned()
        .unwrap_or_else(|| PromptContext {
            kind: PromptContextKind::RetrievedMemories,
            text: render_retrieved_memories(request.retrieved),
            metadata: Value::Null,
        });

    let (template_id, template) = if request.question.category == Some(5) {
        (locomo::QA_PROMPT_CAT_5_TEMPLATE_ID, locomo::QA_PROMPT_CAT_5)
    } else {
        (locomo::QA_PROMPT_TEMPLATE_ID, locomo::QA_PROMPT)
    };

    let user_prompt = format!(
        "Context:\n{}\n\n{}",
        resolved_context.text,
        template.replacen("{}", &request.question.question, 1),
    );

    Ok(PreparedPrompt {
        system_prompt: None,
        user_prompt,
        template_id: template_id.to_string(),
        metadata: json!({
            "requested_profile": Value::Null,
            "resolved_profile": Value::Null,
            "context_kind": resolved_context.kind,
        }),
    })
}

fn build_longmemeval_prompt(request: PromptBuildRequest<'_>) -> anyhow::Result<PreparedPrompt> {
    let config = request
        .longmemeval_prompt
        .context("LongMemEval prompt config is required to build answer prompts")?;
    let resolved_context = resolve_longmemeval_context(
        config.answer_profile,
        request.prompt_context,
        request.retrieved,
    )?;
    let current_date = request
        .question
        .metadata
        .get("raw_question_date")
        .and_then(Value::as_str)
        .or(request.question.question_timestamp.as_deref())
        .context("LongMemEval question is missing prompt-ready question date metadata")?;
    let user_prompt = longmemeval::render_prompt(
        config.answer_profile,
        config.cot,
        &resolved_context.text,
        current_date,
        &request.question.question,
    );

    Ok(PreparedPrompt {
        system_prompt: None,
        user_prompt,
        template_id: longmemeval::template_id(config.answer_profile, config.cot).to_string(),
        metadata: json!({
            "requested_profile": config.answer_profile.as_str(),
            "resolved_profile": config.answer_profile.as_str(),
            "context_kind": resolved_context.kind,
        }),
    })
}

fn resolve_longmemeval_context(
    requested_profile: LongMemEvalAnswerPromptProfile,
    prompt_context: Option<&PromptContext>,
    retrieved: &[RetrievedMemory],
) -> anyhow::Result<PromptContext> {
    match requested_profile {
        LongMemEvalAnswerPromptProfile::NoRetrieval => {
            if let Some(context) = prompt_context {
                ensure!(
                    context.kind == PromptContextKind::NoRetrieval,
                    "LongMemEval no-retrieval prompt requires NoRetrieval context, got {:?}",
                    context.kind
                );
                Ok(context.clone())
            } else {
                Ok(PromptContext {
                    kind: PromptContextKind::NoRetrieval,
                    text: String::new(),
                    metadata: Value::Null,
                })
            }
        }
        LongMemEvalAnswerPromptProfile::HistoryChats => {
            if let Some(context) = prompt_context {
                ensure!(
                    context.kind == PromptContextKind::HistoryChats,
                    "LongMemEval history-chats prompt requires HistoryChats context, got {:?}",
                    context.kind
                );
                Ok(context.clone())
            } else {
                Ok(PromptContext {
                    kind: PromptContextKind::HistoryChats,
                    text: render_retrieved_memories(retrieved),
                    metadata: Value::Null,
                })
            }
        }
        LongMemEvalAnswerPromptProfile::HistoryChatsWithFacts => {
            let context = prompt_context.context(
                "LongMemEval history-chats-with-facts prompt requires backend-provided prompt context",
            )?;
            ensure!(
                context.kind == PromptContextKind::HistoryChatsWithFacts,
                "LongMemEval history-chats-with-facts prompt requires HistoryChatsWithFacts context, got {:?}",
                context.kind
            );
            Ok(context.clone())
        }
        LongMemEvalAnswerPromptProfile::FactsOnly => {
            let context = prompt_context.context(
                "LongMemEval facts-only prompt requires backend-provided prompt context",
            )?;
            ensure!(
                context.kind == PromptContextKind::FactsOnly,
                "LongMemEval facts-only prompt requires FactsOnly context, got {:?}",
                context.kind
            );
            Ok(context.clone())
        }
    }
}

fn render_retrieved_memories(retrieved: &[RetrievedMemory]) -> String {
    if retrieved.is_empty() {
        return "(none)".to_string();
    }

    retrieved
        .iter()
        .enumerate()
        .map(|(index, memory)| {
            let speaker = memory
                .speaker_name
                .as_deref()
                .or(memory.speaker_id.as_deref())
                .unwrap_or("unknown-speaker");
            format!(
                "{}. [stream={} timestamp={} speaker={}]\n{}",
                index + 1,
                memory.stream_id,
                memory.timestamp,
                speaker,
                memory.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::{
        DefaultPromptBuilder, LongMemEvalAnswerPromptProfile, LongMemEvalPromptConfig,
        PromptBuildRequest, PromptBuilder,
    };
    use crate::model::{
        BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, GoldAnswerVariant, RetrievedMemory,
    };
    use crate::prompt::{PromptContext, PromptContextKind};

    #[test]
    fn locomo_category_1_4_uses_default_template() {
        let builder = DefaultPromptBuilder;
        let case = sample_case(BenchmarkDataset::LoCoMo);
        let question = sample_question(BenchmarkDataset::LoCoMo, Some(4));

        let prompt = builder
            .build_answer_prompt(PromptBuildRequest {
                dataset: BenchmarkDataset::LoCoMo,
                case: &case,
                question: &question,
                retrieved: &sample_retrieved(),
                prompt_context: None,
                longmemeval_prompt: None,
            })
            .unwrap();

        assert_eq!(prompt.template_id, "locomo.qa.default.v1");
        assert!(
            prompt
                .user_prompt
                .contains("write an answer in the form of a short phrase")
        );
    }

    #[test]
    fn locomo_category_5_uses_adversarial_template() {
        let builder = DefaultPromptBuilder;
        let case = sample_case(BenchmarkDataset::LoCoMo);
        let question = sample_question(BenchmarkDataset::LoCoMo, Some(5));

        let prompt = builder
            .build_answer_prompt(PromptBuildRequest {
                dataset: BenchmarkDataset::LoCoMo,
                case: &case,
                question: &question,
                retrieved: &sample_retrieved(),
                prompt_context: None,
                longmemeval_prompt: None,
            })
            .unwrap();

        assert_eq!(prompt.template_id, "locomo.qa.cat5.v1");
        assert!(
            prompt
                .user_prompt
                .contains("Based on the above context, answer the following question.")
        );
    }

    #[test]
    fn longmemeval_no_retrieval_uses_no_retrieval_template() {
        let prompt =
            build_longmemeval_prompt(LongMemEvalAnswerPromptProfile::NoRetrieval, false, None);

        assert_eq!(prompt.template_id, "longmemeval.answer.no_retrieval.v1");
        assert!(!prompt.user_prompt.contains("History Chats:"));
    }

    #[test]
    fn longmemeval_no_retrieval_rejects_wrong_context_kind() {
        let builder = DefaultPromptBuilder;
        let case = sample_case(BenchmarkDataset::LongMemEval);
        let question = sample_question(BenchmarkDataset::LongMemEval, None);
        let context = PromptContext {
            kind: PromptContextKind::HistoryChats,
            text: "### Session 1".to_string(),
            metadata: serde_json::Value::Null,
        };

        let error = builder
            .build_answer_prompt(PromptBuildRequest {
                dataset: BenchmarkDataset::LongMemEval,
                case: &case,
                question: &question,
                retrieved: &sample_retrieved(),
                prompt_context: Some(&context),
                longmemeval_prompt: Some(LongMemEvalPromptConfig {
                    answer_profile: LongMemEvalAnswerPromptProfile::NoRetrieval,
                    cot: false,
                }),
            })
            .unwrap_err()
            .to_string();

        assert!(error.contains("NoRetrieval context"));
    }

    #[test]
    fn longmemeval_history_chats_uses_history_template() {
        let prompt = build_longmemeval_prompt(
            LongMemEvalAnswerPromptProfile::HistoryChats,
            false,
            Some(PromptContext {
                kind: PromptContextKind::HistoryChats,
                text: "### Session 1".to_string(),
                metadata: serde_json::Value::Null,
            }),
        );

        assert_eq!(prompt.template_id, "longmemeval.answer.history_chats.v1");
        assert!(prompt.user_prompt.contains("relevant chat history"));
    }

    #[test]
    fn longmemeval_history_chats_with_facts_uses_facts_template() {
        let prompt = build_longmemeval_prompt(
            LongMemEvalAnswerPromptProfile::HistoryChatsWithFacts,
            false,
            Some(PromptContext {
                kind: PromptContextKind::HistoryChatsWithFacts,
                text: "fact summary".to_string(),
                metadata: serde_json::Value::Null,
            }),
        );

        assert_eq!(
            prompt.template_id,
            "longmemeval.answer.history_chats_with_facts.v1"
        );
        assert!(prompt.user_prompt.contains("relevant user facts extracted"));
    }

    #[test]
    fn longmemeval_facts_only_uses_facts_only_template() {
        let prompt = build_longmemeval_prompt(
            LongMemEvalAnswerPromptProfile::FactsOnly,
            false,
            Some(PromptContext {
                kind: PromptContextKind::FactsOnly,
                text: "fact: user moved".to_string(),
                metadata: serde_json::Value::Null,
            }),
        );

        assert_eq!(prompt.template_id, "longmemeval.answer.facts_only.v1");
        assert!(prompt.user_prompt.contains("based on the relevant facts"));
    }

    #[test]
    fn longmemeval_cot_templates_include_step_by_step_instruction() {
        let prompt = build_longmemeval_prompt(
            LongMemEvalAnswerPromptProfile::HistoryChats,
            true,
            Some(PromptContext {
                kind: PromptContextKind::HistoryChats,
                text: "### Session 1".to_string(),
                metadata: serde_json::Value::Null,
            }),
        );

        assert!(prompt.user_prompt.contains("Answer (step by step):"));
        assert!(
            prompt
                .user_prompt
                .contains("first extract all the relevant information")
        );
    }

    #[test]
    fn longmemeval_prompt_includes_current_date() {
        let prompt = build_longmemeval_prompt(
            LongMemEvalAnswerPromptProfile::HistoryChats,
            false,
            Some(PromptContext {
                kind: PromptContextKind::HistoryChats,
                text: "### Session 1".to_string(),
                metadata: serde_json::Value::Null,
            }),
        );

        assert!(prompt.user_prompt.contains("Current Date: 2024-01-03"));
    }

    fn build_longmemeval_prompt(
        profile: LongMemEvalAnswerPromptProfile,
        cot: bool,
        prompt_context: Option<PromptContext>,
    ) -> super::PreparedPrompt {
        let builder = DefaultPromptBuilder;
        let case = sample_case(BenchmarkDataset::LongMemEval);
        let question = sample_question(BenchmarkDataset::LongMemEval, None);

        builder
            .build_answer_prompt(PromptBuildRequest {
                dataset: BenchmarkDataset::LongMemEval,
                case: &case,
                question: &question,
                retrieved: &sample_retrieved(),
                prompt_context: prompt_context.as_ref(),
                longmemeval_prompt: Some(LongMemEvalPromptConfig {
                    answer_profile: profile,
                    cot,
                }),
            })
            .unwrap()
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
            event_id: "event-1".to_string(),
            stream_id: "session-1".to_string(),
            timestamp: "2024-01-01T10:00:00Z".to_string(),
            content: "The user moved to Kyoto.".to_string(),
            speaker_id: Some("user".to_string()),
            speaker_name: Some("User".to_string()),
            metadata: serde_json::Value::Null,
        }]
    }
}
