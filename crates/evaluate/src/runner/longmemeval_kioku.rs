use crate::answerer::Answerer;
use crate::backend::MemoryBackend;
use crate::config::PromptConfig;
use crate::judge::{AnswerJudge, RetrievalJudge};
use crate::model::{BenchmarkCase, RetrievalBudget};
use crate::prompt::PromptBuilder;
use crate::token_counter::TokenCounter;
use anyhow::Context;

use super::policy::ContextTokenPolicy;
use super::result::EvaluatePipelineResult;
use super::{CommonEvaluatePipeline, LongMemEvalKiokuEvaluationProtocol};

pub struct LongMemEvalKiokuEvaluatePipeline<
    'a,
    B: ?Sized,
    P: ?Sized,
    A: ?Sized,
    AJ: ?Sized,
    RJ: ?Sized,
> {
    pub backend: &'a mut B,
    pub prompt_builder: &'a P,
    pub answerer: &'a A,
    pub answer_judge: &'a AJ,
    pub retrieval_judge: &'a RJ,
    pub token_counter: &'a dyn TokenCounter,
    pub budget: RetrievalBudget,
    pub prompt_config: PromptConfig,
}

impl<'a, B, P, A, AJ, RJ> LongMemEvalKiokuEvaluatePipeline<'a, B, P, A, AJ, RJ>
where
    B: MemoryBackend + ?Sized,
    P: PromptBuilder + ?Sized,
    A: Answerer + ?Sized,
    AJ: AnswerJudge + ?Sized,
    RJ: RetrievalJudge + ?Sized,
{
    pub const fn context_token_policy() -> ContextTokenPolicy {
        ContextTokenPolicy::Required
    }

    pub async fn run(&mut self, cases: &[BenchmarkCase]) -> anyhow::Result<EvaluatePipelineResult> {
        let protocol = LongMemEvalKiokuEvaluationProtocol::new(
            self.prompt_config.longmemeval_kioku.as_ref().context(
                "LongMemEval longmemeval_kioku pipeline requires prompt.longmemeval_kioku configuration",
            )?,
        );
        let mut pipeline = CommonEvaluatePipeline {
            backend: self.backend,
            prompt_builder: self.prompt_builder,
            answerer: self.answerer,
            answer_judge: self.answer_judge,
            retrieval_judge: self.retrieval_judge,
            token_counter: Some(self.token_counter),
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
    use crate::prompt::{DefaultPromptBuilder, LongMemEvalKiokuPromptConfig, PromptContext};
    use crate::token_counter::WhitespaceTokenCounter;

    use super::LongMemEvalKiokuEvaluatePipeline;
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
                    "judge_kind": "longmemeval_kioku_answer_llm",
                    "judge_model": "judge-model",
                    "judge_prompt_id": "longmemeval.kioku.judge.answer.v1",
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
                    "judge_kind": "longmemeval_kioku_retrieval_llm",
                    "judge_model": "judge-model",
                    "judge_prompt_id": "longmemeval.kioku.judge.retrieval.v1",
                    "supported_answer": "Kyoto",
                    "reason": "stub",
                }),
            })
        }
    }

    fn sample_case(is_abstention: bool) -> BenchmarkCase {
        BenchmarkCase {
            dataset: BenchmarkDataset::LongMemEval,
            case_id: "longmemeval:sample".to_string(),
            events: vec![BenchmarkEvent {
                event_id: "e1".to_string(),
                stream_id: "session_1".to_string(),
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                content: "I moved to Kyoto.".to_string(),
                speaker_id: Some("user".to_string()),
                speaker_name: Some("user".to_string()),
                metadata: serde_json::json!({
                    "session_id": "session_1",
                    "session_date": "2024-01-01",
                }),
            }],
            questions: vec![BenchmarkQuestion {
                question_id: "q1".to_string(),
                question: "Where does the user live now?".to_string(),
                question_timestamp: Some("2024-01-03T00:00:00Z".to_string()),
                gold_answers: vec!["Kyoto".to_string()],
                evidence_event_ids: vec!["e1".to_string()],
                evidence_session_ids: vec!["session_1".to_string()],
                category: None,
                question_type: Some("knowledge-update".to_string()),
                gold_answer_variant: GoldAnswerVariant::Default,
                is_abstention,
                metadata: serde_json::json!({
                    "raw_question_date": "2024-01-03",
                }),
            }],
            metadata: serde_json::Value::Null,
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

    #[tokio::test]
    async fn retrieval_judge_and_answerer_share_same_context_text() {
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = DefaultPromptBuilder;
        let answerer = ContextEchoAnswerer::default();
        let answer_judge = RecordingAnswerJudge;
        let retrieval_judge = RecordingRetrievalJudge::default();
        let seen_contexts = retrieval_judge.seen_contexts.clone();
        let token_counter = WhitespaceTokenCounter;

        let mut pipeline = LongMemEvalKiokuEvaluatePipeline {
            backend: &mut backend,
            prompt_builder: &prompt_builder,
            answerer: &answerer,
            answer_judge: &answer_judge,
            retrieval_judge: &retrieval_judge,
            token_counter: &token_counter,
            budget: RetrievalBudget::default(),
            prompt_config: crate::config::PromptConfig {
                longmemeval_kioku: Some(LongMemEvalKiokuPromptConfig {
                    answer_template_id: "longmemeval.kioku.answer.v1".to_string(),
                    answer_judge_prompt_id: "longmemeval.kioku.judge.answer.v1".to_string(),
                    retrieval_judge_prompt_id: "longmemeval.kioku.judge.retrieval.v1".to_string(),
                }),
                locomo_kioku: None,
            },
        };

        pipeline.run(&[sample_case(false)]).await.unwrap();

        let retrieval_context = seen_contexts.lock().unwrap()[0].clone();
        let answer_prompt = answerer.prompts.lock().unwrap()[0].clone();
        assert!(answer_prompt.contains(&retrieval_context));
    }

    #[tokio::test]
    async fn longmemeval_metrics_exclude_abstention_from_main_scores() {
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = DefaultPromptBuilder;
        let answerer = DebugAnswerer::new("correct");
        let answer_judge = RecordingAnswerJudge;
        let retrieval_judge = RecordingRetrievalJudge::default();
        let token_counter = WhitespaceTokenCounter;

        let mut pipeline = LongMemEvalKiokuEvaluatePipeline {
            backend: &mut backend,
            prompt_builder: &prompt_builder,
            answerer: &answerer,
            answer_judge: &answer_judge,
            retrieval_judge: &retrieval_judge,
            token_counter: &token_counter,
            budget: RetrievalBudget::default(),
            prompt_config: crate::config::PromptConfig {
                longmemeval_kioku: Some(LongMemEvalKiokuPromptConfig {
                    answer_template_id: "longmemeval.kioku.answer.v1".to_string(),
                    answer_judge_prompt_id: "longmemeval.kioku.judge.answer.v1".to_string(),
                    retrieval_judge_prompt_id: "longmemeval.kioku.judge.retrieval.v1".to_string(),
                }),
                locomo_kioku: None,
            },
        };

        let result = pipeline
            .run(&[sample_case(false), sample_case(true)])
            .await
            .unwrap();

        assert_eq!(result.metrics.metrics.question_count, 2);
        assert_eq!(
            result.metrics.metrics.non_abstention_question_count,
            Some(1)
        );
        assert_eq!(result.metrics.metrics.abstention_question_count, Some(1));
        assert_eq!(result.metrics.metrics.overall_answer_accuracy, Some(1.0));
        assert_eq!(result.metrics.metrics.abstention_answer_accuracy, Some(1.0));
    }

    #[test]
    fn longmemeval_context_token_policy_is_required() {
        assert_eq!(
            LongMemEvalKiokuEvaluatePipeline::<
                crate::backend::ReturnAllMemoryBackend,
                crate::prompt::DefaultPromptBuilder,
                crate::answerer::DebugAnswerer,
                RecordingAnswerJudge,
                RecordingRetrievalJudge,
            >::context_token_policy(),
            ContextTokenPolicy::Required
        );
    }
}
