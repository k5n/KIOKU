use anyhow::Context;

use crate::model::{BenchmarkDataset, BenchmarkQuestion, MetricsReport};
use crate::prompt::{AnswerPromptProfile, LongMemEvalKiokuPromptConfig};

use super::{DatasetEvaluationProtocol, EvaluatedQuestion};
use crate::runner::ContextTokenPolicy;
use crate::runner::metrics::{LongMemEvalKiokuMetricInput, build_longmemeval_kioku_metrics};

#[derive(Debug, Clone, Copy)]
pub(crate) struct LongMemEvalKiokuEvaluationProtocol<'a> {
    prompt: &'a LongMemEvalKiokuPromptConfig,
}

impl<'a> LongMemEvalKiokuEvaluationProtocol<'a> {
    pub const fn new(prompt: &'a LongMemEvalKiokuPromptConfig) -> Self {
        Self { prompt }
    }
}

impl DatasetEvaluationProtocol for LongMemEvalKiokuEvaluationProtocol<'_> {
    type MetricInput = LongMemEvalKiokuMetricInput;

    fn dataset(&self) -> BenchmarkDataset {
        BenchmarkDataset::LongMemEval
    }

    fn context_token_policy(&self) -> ContextTokenPolicy {
        ContextTokenPolicy::Required
    }

    fn include_question(&self, _question: &BenchmarkQuestion) -> bool {
        true
    }

    fn answer_prompt_profile<'a>(&'a self) -> AnswerPromptProfile<'a> {
        AnswerPromptProfile::LongMemEvalKioku(self.prompt)
    }

    fn build_metric_input(
        &self,
        evaluated: &EvaluatedQuestion<'_>,
    ) -> anyhow::Result<Self::MetricInput> {
        Ok(LongMemEvalKiokuMetricInput {
            question_type: evaluated
                .question
                .question_type
                .clone()
                .context("LongMemEval Kioku metrics require question_type")?,
            is_abstention: evaluated.question.is_abstention,
            answer: evaluated.answer_judgement.clone(),
            retrieval: evaluated.retrieval_judgement.clone(),
            context_token_count: evaluated
                .context_token_count
                .context("LongMemEval Kioku metrics require context_token_count")?,
            answerer_model: evaluated.answerer_model.clone(),
        })
    }

    fn build_metrics(
        &self,
        inputs: &[Self::MetricInput],
        context_tokenizer: Option<&str>,
    ) -> anyhow::Result<MetricsReport> {
        Ok(build_longmemeval_kioku_metrics(
            inputs,
            &self.prompt.answer_judge_prompt_id,
            &self.prompt.retrieval_judge_prompt_id,
            context_tokenizer.context(
                "LongMemEval Kioku metrics require a context_tokenizer provenance value",
            )?,
        ))
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
    use crate::runner::{ContextTokenPolicy, run_pipeline};
    use crate::token_counter::WhitespaceTokenCounter;

    use super::{DatasetEvaluationProtocol, LongMemEvalKiokuEvaluationProtocol};

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

    fn sample_prompt_config() -> LongMemEvalKiokuPromptConfig {
        LongMemEvalKiokuPromptConfig {
            answer_template_id: "longmemeval.kioku.answer.v1".to_string(),
            answer_judge_prompt_id: "longmemeval.kioku.judge.answer.v1".to_string(),
            retrieval_judge_prompt_id: "longmemeval.kioku.judge.retrieval.v1".to_string(),
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

    #[test]
    fn longmemeval_context_token_policy_is_required() {
        let prompt = sample_prompt_config();
        let protocol = LongMemEvalKiokuEvaluationProtocol::new(&prompt);

        assert_eq!(
            protocol.context_token_policy(),
            ContextTokenPolicy::Required
        );
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
        let prompt = sample_prompt_config();
        let protocol = LongMemEvalKiokuEvaluationProtocol::new(&prompt);
        run_pipeline(
            &[sample_case(false)],
            &mut backend,
            &prompt_builder,
            &answerer,
            &answer_judge,
            &retrieval_judge,
            Some(&token_counter),
            RetrievalBudget::default(),
            &protocol,
        )
        .await
        .unwrap();

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
        let prompt = sample_prompt_config();
        let protocol = LongMemEvalKiokuEvaluationProtocol::new(&prompt);
        let result = run_pipeline(
            &[sample_case(false), sample_case(true)],
            &mut backend,
            &prompt_builder,
            &answerer,
            &answer_judge,
            &retrieval_judge,
            Some(&token_counter),
            RetrievalBudget::default(),
            &protocol,
        )
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
}
