use crate::answerer::Answerer;
use crate::backend::MemoryBackend;
use crate::config::PromptConfig;
use crate::judge::{AnswerJudge, RetrievalJudge};
use crate::model::{
    AnswerLogRecord, BenchmarkCase, BenchmarkDataset, EvalScope, QueryInput, RetrievalBudget,
    RetrievalLogRecord,
};
use crate::prompt::{PromptBuildRequest, PromptBuilder};
use crate::token_counter::TokenCounter;
use anyhow::{Context, ensure};

use super::helpers::{context_kind_name, extract_answerer_model, sanitize_answer_metadata};
use super::metrics::{LongMemEvalKiokuMetricInput, build_longmemeval_kioku_metrics};
use super::result::EvaluatePipelineResult;

pub struct LongMemEvalKiokuEvaluatePipeline<
    'a,
    B: ?Sized,
    P: ?Sized,
    A: ?Sized,
    AJ: ?Sized,
    RJ: ?Sized,
    TC: ?Sized,
> {
    pub backend: &'a mut B,
    pub prompt_builder: &'a P,
    pub answerer: &'a A,
    pub answer_judge: &'a AJ,
    pub retrieval_judge: &'a RJ,
    pub token_counter: &'a TC,
    pub budget: RetrievalBudget,
    pub prompt_config: PromptConfig,
}

impl<'a, B, P, A, AJ, RJ, TC> LongMemEvalKiokuEvaluatePipeline<'a, B, P, A, AJ, RJ, TC>
where
    B: MemoryBackend + ?Sized,
    P: PromptBuilder + ?Sized,
    A: Answerer + ?Sized,
    AJ: AnswerJudge + ?Sized,
    RJ: RetrievalJudge + ?Sized,
    TC: TokenCounter + ?Sized,
{
    pub async fn run(&mut self, cases: &[BenchmarkCase]) -> anyhow::Result<EvaluatePipelineResult> {
        let dataset = cases
            .first()
            .map(|case| case.dataset)
            .context("evaluate pipeline requires at least one case")?;
        ensure!(
            dataset == BenchmarkDataset::LongMemEval,
            "LongMemEvalKiokuEvaluatePipeline only supports LongMemEval cases"
        );
        ensure!(
            cases.iter().all(|case| case.dataset == dataset),
            "evaluate pipeline requires all cases to use the same dataset"
        );

        let prompt = self.prompt_config.longmemeval_kioku.as_ref().context(
            "LongMemEval longmemeval_kioku pipeline requires prompt.longmemeval_kioku configuration",
        )?;

        let mut answers = Vec::new();
        let mut retrievals = Vec::new();
        let mut metric_inputs = Vec::new();

        for case in cases {
            self.backend
                .reset(EvalScope {
                    dataset,
                    case_id: case.case_id.clone(),
                })
                .await?;

            for event in &case.events {
                self.backend.ingest(event.clone()).await?;
            }

            for question in &case.questions {
                let query_output = self
                    .backend
                    .query(QueryInput {
                        scope: EvalScope {
                            dataset,
                            case_id: case.case_id.clone(),
                        },
                        question_id: question.question_id.clone(),
                        query: question.question.clone(),
                        timestamp: question.question_timestamp.clone(),
                        budget: self.budget,
                        metadata: serde_json::Value::Null,
                    })
                    .await?;
                let prompt_context = &query_output.prompt_context;
                let retrieval_judgement = self
                    .retrieval_judge
                    .judge_retrieval(question, prompt_context)
                    .await?;
                let prepared_prompt =
                    self.prompt_builder
                        .build_answer_prompt(PromptBuildRequest {
                            dataset,
                            case,
                            question,
                            prompt_context,
                            locomo_kioku_prompt: None,
                            longmemeval_kioku_prompt: Some(prompt),
                        })?;
                let generated = self
                    .answerer
                    .answer(crate::model::AnswerRequest {
                        prompt: &prepared_prompt,
                    })
                    .await?;
                let answer_judgement = self.answer_judge.judge_answer(question, &generated).await?;
                let answerer_model = extract_answerer_model(&generated.metadata);
                let context_token_count =
                    self.token_counter.count_text_tokens(&prompt_context.text)?;

                retrievals.push(RetrievalLogRecord {
                    dataset,
                    case_id: case.case_id.clone(),
                    question_id: question.question_id.clone(),
                    category: question.category,
                    context_kind: Some(context_kind_name(prompt_context)),
                    context_text: Some(prompt_context.text.clone()),
                    is_sufficient: Some(retrieval_judgement.passed),
                    score: Some(retrieval_judgement.score),
                    label: Some(retrieval_judgement.label.clone()),
                    judge_metadata: retrieval_judgement.metadata.clone(),
                    evidence_event_ids: question.evidence_event_ids.clone(),
                    evidence_session_ids: question.evidence_session_ids.clone(),
                    metadata: query_output.metadata,
                });

                answers.push(AnswerLogRecord {
                    dataset,
                    case_id: case.case_id.clone(),
                    question_id: question.question_id.clone(),
                    question: question.question.clone(),
                    generated_answer: generated.text,
                    gold_answers: question.gold_answers.clone(),
                    is_correct: answer_judgement.passed,
                    score: answer_judgement.score,
                    label: answer_judgement.label.clone(),
                    question_type: question.question_type.clone(),
                    category: question.category,
                    is_abstention: question.is_abstention,
                    answer_metadata: sanitize_answer_metadata(
                        generated.metadata,
                        &prepared_prompt.template_id,
                        &answerer_model,
                    ),
                    judgement_metadata: answer_judgement.metadata.clone(),
                });

                metric_inputs.push(LongMemEvalKiokuMetricInput {
                    question_type: question
                        .question_type
                        .clone()
                        .context("LongMemEval Kioku metrics require question_type")?,
                    is_abstention: question.is_abstention,
                    answer: answer_judgement,
                    retrieval: retrieval_judgement,
                    context_token_count,
                    answerer_model,
                });
            }
        }

        let metrics = build_longmemeval_kioku_metrics(
            &metric_inputs,
            &prompt.answer_judge_prompt_id,
            &prompt.retrieval_judge_prompt_id,
            self.token_counter.name(),
        );

        Ok(EvaluatePipelineResult {
            answers,
            retrievals,
            metrics,
        })
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
}
