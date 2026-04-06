use async_trait::async_trait;

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
        Ok(GeneratedAnswer {
            text: self.fixed_answer.clone(),
            metadata: serde_json::json!({
                "answerer_kind": "debug",
                "mode": "fixed",
                "question_id": request.question.question_id,
                "retrieved_count": request.retrieved.len(),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::DebugAnswerer;
    use crate::answerer::Answerer;
    use crate::model::{
        AnswerRequest, BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, GoldAnswerVariant,
    };

    #[tokio::test]
    async fn returns_fixed_answer_with_debug_metadata() {
        let answerer = DebugAnswerer::default();
        let case = BenchmarkCase {
            dataset: BenchmarkDataset::LoCoMo,
            case_id: "locomo:sample".to_string(),
            events: Vec::new(),
            questions: Vec::new(),
            metadata: serde_json::Value::Null,
        };
        let question = BenchmarkQuestion {
            question_id: "locomo:sample:q0".to_string(),
            question: "What happened?".to_string(),
            question_timestamp: None,
            gold_answers: vec!["gold".to_string()],
            evidence_event_ids: Vec::new(),
            evidence_session_ids: Vec::new(),
            category: Some(1),
            question_type: None,
            gold_answer_variant: GoldAnswerVariant::Default,
            is_abstention: false,
            metadata: serde_json::Value::Null,
        };

        let generated = answerer
            .answer(AnswerRequest {
                dataset: BenchmarkDataset::LoCoMo,
                case: &case,
                question: &question,
                retrieved: &[],
            })
            .await
            .unwrap();

        assert_eq!(generated.text, "[debug-answer]");
        assert_eq!(generated.metadata["answerer_kind"], "debug");
        assert_eq!(generated.metadata["question_id"], "locomo:sample:q0");
    }
}
