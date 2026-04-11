use crate::answerer::Answerer;
use crate::backend::MemoryBackend;
use crate::config::PromptConfig;
use crate::judge::{AnswerJudge, RetrievalJudge};
use crate::model::{BenchmarkCase, RetrievalBudget};
use crate::prompt::PromptBuilder;
use anyhow::Context;

use super::policy::ContextTokenPolicy;
use super::result::EvaluatePipelineResult;
use super::{CommonEvaluatePipeline, LoCoMoKiokuEvaluationProtocol};

pub struct LoCoMoKiokuEvaluatePipeline<'a, B: ?Sized, P: ?Sized, A: ?Sized, AJ: ?Sized, RJ: ?Sized>
{
    pub backend: &'a mut B,
    pub prompt_builder: &'a P,
    pub answerer: &'a A,
    pub answer_judge: &'a AJ,
    pub retrieval_judge: &'a RJ,
    pub budget: RetrievalBudget,
    pub prompt_config: PromptConfig,
}

impl<'a, B, P, A, AJ, RJ> LoCoMoKiokuEvaluatePipeline<'a, B, P, A, AJ, RJ>
where
    B: MemoryBackend + ?Sized,
    P: PromptBuilder + ?Sized,
    A: Answerer + ?Sized,
    AJ: AnswerJudge + ?Sized,
    RJ: RetrievalJudge + ?Sized,
{
    pub const fn context_token_policy() -> ContextTokenPolicy {
        ContextTokenPolicy::Optional
    }

    pub async fn run(&mut self, cases: &[BenchmarkCase]) -> anyhow::Result<EvaluatePipelineResult> {
        let protocol =
            LoCoMoKiokuEvaluationProtocol::new(self.prompt_config.locomo_kioku.as_ref().context(
                "LoCoMo locomo_kioku pipeline requires prompt.locomo_kioku configuration",
            )?);
        let mut pipeline = CommonEvaluatePipeline {
            backend: self.backend,
            prompt_builder: self.prompt_builder,
            answerer: self.answerer,
            answer_judge: self.answer_judge,
            retrieval_judge: self.retrieval_judge,
            token_counter: None,
            budget: self.budget,
            protocol: &protocol,
        };
        pipeline.run(cases).await
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    use crate::answerer::{Answerer, DebugAnswerer};
    use crate::backend::ReturnAllMemoryBackend;
    use crate::judge::{AnswerJudge, BinaryJudgement, RetrievalJudge};
    use crate::model::{
        AnswerRequest, BenchmarkCase, BenchmarkDataset, BenchmarkEvent, BenchmarkQuestion,
        GeneratedAnswer, GoldAnswerVariant, RetrievalBudget,
    };
    use crate::prompt::{DefaultPromptBuilder, LocomoKiokuPromptConfig, PromptContext};

    use super::LoCoMoKiokuEvaluatePipeline;
    use crate::runner::ContextTokenPolicy;

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

    #[tokio::test]
    async fn locomo_pipeline_skips_category_five_from_logs_and_metrics() {
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = DefaultPromptBuilder;
        let answerer = DebugAnswerer::new("correct");
        let answer_judge = RecordingAnswerJudge;
        let retrieval_judge = RecordingRetrievalJudge::default();
        let mut pipeline = LoCoMoKiokuEvaluatePipeline {
            backend: &mut backend,
            prompt_builder: &prompt_builder,
            answerer: &answerer,
            answer_judge: &answer_judge,
            retrieval_judge: &retrieval_judge,
            budget: RetrievalBudget::default(),
            prompt_config: crate::config::PromptConfig {
                longmemeval_kioku: None,
                locomo_kioku: Some(LocomoKiokuPromptConfig {
                    answer_template_id: "locomo.kioku.answer.v1".to_string(),
                    answer_judge_prompt_id: "locomo.kioku.judge.answer.v1".to_string(),
                    retrieval_judge_prompt_id: "locomo.kioku.judge.retrieval.v1".to_string(),
                }),
            },
        };

        let result = pipeline.run(&[sample_case()]).await.unwrap();

        assert_eq!(result.answers.len(), 1);
        assert_eq!(result.retrievals.len(), 1);
        assert_eq!(result.metrics.metrics.question_count, 1);
        assert_eq!(result.metrics.metrics.overall_answer_accuracy, Some(1.0));
    }

    #[derive(Debug, Default)]
    struct ContextEchoAnswerer {
        contexts: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl Answerer for ContextEchoAnswerer {
        async fn answer(&self, request: AnswerRequest<'_>) -> anyhow::Result<GeneratedAnswer> {
            self.contexts
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

    #[tokio::test]
    async fn retrieval_judge_and_answerer_share_same_context_text() {
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = DefaultPromptBuilder;
        let answerer = ContextEchoAnswerer::default();
        let answer_judge = RecordingAnswerJudge;
        let retrieval_judge = RecordingRetrievalJudge::default();
        let seen_contexts = retrieval_judge.seen_contexts.clone();

        let mut pipeline = LoCoMoKiokuEvaluatePipeline {
            backend: &mut backend,
            prompt_builder: &prompt_builder,
            answerer: &answerer,
            answer_judge: &answer_judge,
            retrieval_judge: &retrieval_judge,
            budget: RetrievalBudget::default(),
            prompt_config: crate::config::PromptConfig {
                longmemeval_kioku: None,
                locomo_kioku: Some(LocomoKiokuPromptConfig {
                    answer_template_id: "locomo.kioku.answer.v1".to_string(),
                    answer_judge_prompt_id: "locomo.kioku.judge.answer.v1".to_string(),
                    retrieval_judge_prompt_id: "locomo.kioku.judge.retrieval.v1".to_string(),
                }),
            },
        };

        pipeline.run(&[sample_case()]).await.unwrap();

        let retrieval_context = seen_contexts.lock().unwrap()[0].clone();
        let answer_prompt = answerer.contexts.lock().unwrap()[0].clone();
        assert!(answer_prompt.contains(&retrieval_context));
    }

    #[test]
    fn locomo_context_token_policy_is_optional() {
        assert_eq!(
            LoCoMoKiokuEvaluatePipeline::<
                crate::backend::ReturnAllMemoryBackend,
                crate::prompt::DefaultPromptBuilder,
                crate::answerer::DebugAnswerer,
                RecordingAnswerJudge,
                RecordingRetrievalJudge,
            >::context_token_policy(),
            ContextTokenPolicy::Optional
        );
    }
}
