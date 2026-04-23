use anyhow::Context;

use crate::common::{
    model::{BenchmarkDataset, BenchmarkQuestion, MetricsReport},
    runner::{ContextTokenPolicy, DatasetEvaluationProtocol, EvaluatedQuestion},
};

use super::{
    config::LocomoKiokuPromptConfig,
    metrics::{LoCoMoKiokuMetricInput, build_metrics},
};

#[derive(Debug, Clone)]
pub(crate) struct LoCoMoKiokuEvaluationProtocol {
    prompt: LocomoKiokuPromptConfig,
}

impl LoCoMoKiokuEvaluationProtocol {
    pub(crate) fn new(prompt: LocomoKiokuPromptConfig) -> Self {
        Self { prompt }
    }
}

impl DatasetEvaluationProtocol for LoCoMoKiokuEvaluationProtocol {
    type MetricInput = LoCoMoKiokuMetricInput;

    fn dataset(&self) -> BenchmarkDataset {
        BenchmarkDataset::LoCoMo
    }

    fn context_token_policy(&self) -> ContextTokenPolicy {
        ContextTokenPolicy::Optional
    }

    fn include_question(&self, question: &BenchmarkQuestion) -> bool {
        matches!(question.category, Some(1..=4))
    }

    fn build_metric_input(
        &self,
        evaluated: &EvaluatedQuestion<'_>,
    ) -> anyhow::Result<Self::MetricInput> {
        Ok(LoCoMoKiokuMetricInput {
            category: evaluated
                .question
                .category
                .context("LoCoMo Kioku metrics require category after protocol filtering")?,
            answer: evaluated.answer_judgement.clone(),
            retrieval: evaluated.retrieval_judgement.clone(),
            answerer_model: evaluated.answerer_model.clone(),
        })
    }

    fn build_metrics(
        &self,
        inputs: &[Self::MetricInput],
        _context_tokenizer: Option<&str>,
    ) -> anyhow::Result<MetricsReport> {
        Ok(build_metrics(
            inputs,
            &self.prompt.answer_judge_prompt_id,
            &self.prompt.retrieval_judge_prompt_id,
        ))
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    use crate::common::{
        answerer::{Answerer, DebugAnswerer},
        backend::ReturnAllMemoryBackend,
        judge::{AnswerJudge, BinaryJudgement, RetrievalJudge},
        model::{
            AnswerRequest, BenchmarkCase, BenchmarkDataset, BenchmarkEvent, BenchmarkQuestion,
            GeneratedAnswer, GoldAnswerVariant, RetrievalBudget,
        },
        prompt::PromptContext,
        runner::{ContextTokenPolicy, run_pipeline},
    };

    use super::{DatasetEvaluationProtocol, LoCoMoKiokuEvaluationProtocol};
    use crate::benchmarks::locomo::{LocomoKiokuPromptConfig, prompt::LocomoPromptBuilder};

    #[derive(Debug, Default)]
    struct RecordingAnswerJudge;

    #[async_trait]
    impl AnswerJudge for RecordingAnswerJudge {
        async fn judge_answer(
            &self,
            _question: &BenchmarkQuestion,
            generated: &GeneratedAnswer,
        ) -> anyhow::Result<BinaryJudgement> {
            Ok(BinaryJudgement {
                passed: generated.text == "correct",
                score: if generated.text == "correct" {
                    1.0
                } else {
                    0.0
                },
                label: if generated.text == "correct" {
                    "CORRECT".to_string()
                } else {
                    "WRONG".to_string()
                },
                metadata: serde_json::json!({
                    "judge_kind": "locomo_kioku_answer_llm",
                    "judge_model": "judge-model",
                    "judge_prompt_id": "locomo.kioku.judge.answer.v1",
                    "reason": "stub",
                }),
            })
        }
    }

    #[derive(Debug, Default)]
    struct RecordingRetrievalJudge {
        seen_contexts: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl RetrievalJudge for RecordingRetrievalJudge {
        async fn judge_retrieval(
            &self,
            _question: &BenchmarkQuestion,
            context: &PromptContext,
        ) -> anyhow::Result<BinaryJudgement> {
            self.seen_contexts
                .lock()
                .unwrap()
                .push(context.text.clone());
            Ok(BinaryJudgement {
                passed: true,
                score: 1.0,
                label: "SUFFICIENT".to_string(),
                metadata: serde_json::json!({
                    "judge_kind": "locomo_kioku_retrieval_llm",
                    "judge_model": "judge-model",
                    "judge_prompt_id": "locomo.kioku.judge.retrieval.v1",
                    "supported_answer": "answer",
                    "reason": "stub",
                }),
            })
        }
    }

    #[derive(Debug, Default)]
    struct ContextEchoAnswerer {
        prompts: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl Answerer for ContextEchoAnswerer {
        async fn answer(&self, request: AnswerRequest<'_>) -> anyhow::Result<GeneratedAnswer> {
            self.prompts
                .lock()
                .unwrap()
                .push(request.prompt.user_prompt.clone());
            Ok(GeneratedAnswer {
                text: "correct".to_string(),
                metadata: serde_json::json!({
                    "answerer": {
                        "kind": "debug",
                    }
                }),
            })
        }
    }

    fn sample_prompt_config() -> LocomoKiokuPromptConfig {
        LocomoKiokuPromptConfig {
            answer_template_id: "locomo.kioku.answer.v1".to_string(),
            answer_judge_prompt_id: "locomo.kioku.judge.answer.v1".to_string(),
            retrieval_judge_prompt_id: "locomo.kioku.judge.retrieval.v1".to_string(),
        }
    }

    fn sample_case() -> BenchmarkCase {
        BenchmarkCase {
            dataset: BenchmarkDataset::LoCoMo,
            case_id: "locomo:sample".to_string(),
            events: vec![BenchmarkEvent {
                event_id: "e1".to_string(),
                stream_id: "session_1".to_string(),
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                content: "The meeting happened in May 2019.".to_string(),
                speaker_id: Some("alice".to_string()),
                speaker_name: Some("alice".to_string()),
                metadata: serde_json::json!({
                    "session_id": "session_1",
                }),
            }],
            questions: vec![
                BenchmarkQuestion {
                    question_id: "q1".to_string(),
                    question: "When was the meeting?".to_string(),
                    question_timestamp: None,
                    gold_answers: vec!["May 2019".to_string()],
                    evidence_event_ids: vec!["e1".to_string()],
                    evidence_session_ids: Vec::new(),
                    category: Some(2),
                    question_type: None,
                    gold_answer_variant: GoldAnswerVariant::Default,
                    is_abstention: false,
                    metadata: serde_json::Value::Null,
                },
                BenchmarkQuestion {
                    question_id: "q2".to_string(),
                    question: "Adversarial?".to_string(),
                    question_timestamp: None,
                    gold_answers: vec!["wrong".to_string()],
                    evidence_event_ids: Vec::new(),
                    evidence_session_ids: Vec::new(),
                    category: Some(5),
                    question_type: None,
                    gold_answer_variant: GoldAnswerVariant::Adversarial,
                    is_abstention: false,
                    metadata: serde_json::Value::Null,
                },
            ],
            metadata: serde_json::Value::Null,
        }
    }

    #[test]
    fn locomo_context_token_policy_is_optional() {
        let protocol = LoCoMoKiokuEvaluationProtocol::new(sample_prompt_config());

        assert_eq!(
            protocol.context_token_policy(),
            ContextTokenPolicy::Optional
        );
    }

    #[tokio::test]
    async fn locomo_pipeline_skips_category_five_from_logs_and_metrics() {
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = LocomoPromptBuilder::new(sample_prompt_config());
        let answerer = DebugAnswerer::new("correct");
        let answer_judge = RecordingAnswerJudge;
        let retrieval_judge = RecordingRetrievalJudge::default();
        let protocol = LoCoMoKiokuEvaluationProtocol::new(sample_prompt_config());
        let result = run_pipeline(
            &[sample_case()],
            &mut backend,
            &prompt_builder,
            &answerer,
            &answer_judge,
            &retrieval_judge,
            None,
            RetrievalBudget::default(),
            &protocol,
        )
        .await
        .unwrap();

        assert_eq!(result.answers.len(), 1);
        assert_eq!(result.retrievals.len(), 1);
        assert_eq!(result.metrics.metrics.question_count, 1);
        assert_eq!(result.metrics.metrics.overall_answer_accuracy, Some(1.0));
    }

    #[tokio::test]
    async fn retrieval_judge_and_answerer_share_same_context_text() {
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = LocomoPromptBuilder::new(sample_prompt_config());
        let answerer = ContextEchoAnswerer::default();
        let answer_judge = RecordingAnswerJudge;
        let retrieval_judge = RecordingRetrievalJudge::default();
        let seen_contexts = retrieval_judge.seen_contexts.clone();
        let protocol = LoCoMoKiokuEvaluationProtocol::new(sample_prompt_config());
        run_pipeline(
            &[sample_case()],
            &mut backend,
            &prompt_builder,
            &answerer,
            &answer_judge,
            &retrieval_judge,
            None,
            RetrievalBudget::default(),
            &protocol,
        )
        .await
        .unwrap();

        let retrieval_context = seen_contexts.lock().unwrap()[0].clone();
        let answer_prompt = answerer.prompts.lock().unwrap()[0].clone();
        assert!(answer_prompt.contains(&retrieval_context));
    }
}
