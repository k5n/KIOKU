use anyhow::{Context, ensure};

use crate::answerer::Answerer;
use crate::backend::MemoryBackend;
use crate::config::PromptConfig;
use crate::judge::Judge;
use crate::model::{
    AnswerLogRecord, AnswerRequest, BenchmarkCase, EvalScope, MetricsReport, QueryInput,
    RetrievalBudget, RetrievalLogRecord,
};
use crate::prompt::{PromptBuildRequest, PromptBuilder};

use super::metrics::build_metrics;

pub struct EvaluatePipeline<'a, B: ?Sized, P: ?Sized, A: ?Sized, J> {
    pub backend: &'a mut B,
    pub prompt_builder: &'a P,
    pub answerer: &'a A,
    pub judge: &'a J,
    pub budget: RetrievalBudget,
    pub prompt_config: PromptConfig,
}

#[derive(Debug)]
pub struct EvaluatePipelineResult {
    pub answers: Vec<AnswerLogRecord>,
    pub retrievals: Vec<RetrievalLogRecord>,
    pub metrics: MetricsReport,
}

impl<'a, B, P, A, J> EvaluatePipeline<'a, B, P, A, J>
where
    B: MemoryBackend + ?Sized,
    P: PromptBuilder + ?Sized,
    A: Answerer + ?Sized,
    J: Judge,
{
    pub async fn run(&mut self, cases: &[BenchmarkCase]) -> anyhow::Result<EvaluatePipelineResult> {
        let dataset = cases
            .first()
            .map(|case| case.dataset)
            .context("evaluate pipeline requires at least one case")?;
        ensure!(
            cases.iter().all(|case| case.dataset == dataset),
            "evaluate pipeline requires all cases to use the same dataset"
        );

        let mut answers = Vec::new();
        let mut retrievals = Vec::new();
        let mut question_judgements = Vec::new();
        let mut retrieved_counts = Vec::new();

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
                        requested_longmemeval_prompt_profile: self
                            .prompt_config
                            .longmemeval
                            .map(|config| config.answer_profile),
                        metadata: serde_json::Value::Null,
                    })
                    .await?;
                let prepared_prompt =
                    self.prompt_builder
                        .build_answer_prompt(PromptBuildRequest {
                            dataset,
                            case,
                            question,
                            retrieved: &query_output.retrieved,
                            prompt_context: query_output.prompt_context.as_ref(),
                            locomo_kioku_prompt: self.prompt_config.locomo_kioku.as_ref(),
                            longmemeval_prompt: self.prompt_config.longmemeval,
                        })?;

                let generated = self
                    .answerer
                    .answer(AnswerRequest {
                        prompt: &prepared_prompt,
                    })
                    .await?;

                let judgement = self.judge.judge(question, &generated).await?;
                retrieved_counts.push(query_output.retrieved.len() as f32);

                retrievals.push(RetrievalLogRecord {
                    dataset,
                    case_id: case.case_id.clone(),
                    question_id: question.question_id.clone(),
                    category: question.category,
                    retrieved_count: query_output.retrieved.len(),
                    retrieved_memory_ids: query_output
                        .retrieved
                        .iter()
                        .map(|memory| memory.memory_id.clone())
                        .collect(),
                    retrieved_source_event_ids: query_output
                        .retrieved
                        .iter()
                        .filter_map(|memory| memory.source_event_id.clone())
                        .collect(),
                    context_kind: query_output.prompt_context.as_ref().map(|context| {
                        serde_json::to_value(&context.kind)
                            .ok()
                            .and_then(|value| value.as_str().map(ToString::to_string))
                            .unwrap_or_else(|| "unknown".to_string())
                    }),
                    context_text: query_output
                        .prompt_context
                        .as_ref()
                        .map(|context| context.text.clone()),
                    is_sufficient: None,
                    score: None,
                    label: None,
                    judge_metadata: serde_json::Value::Null,
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
                    is_correct: judgement.passed,
                    score: judgement.score,
                    label: judgement.label.clone(),
                    question_type: question.question_type.clone(),
                    category: question.category,
                    is_abstention: question.is_abstention,
                    answer_metadata: generated.metadata,
                    judgement_metadata: judgement.metadata.clone(),
                });

                question_judgements.push((question, judgement));
            }
        }

        let metrics = build_metrics(dataset, &question_judgements, &retrieved_counts);

        Ok(EvaluatePipelineResult {
            answers,
            retrievals,
            metrics,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::EvaluatePipeline;
    use crate::answerer::DebugAnswerer;
    use crate::backend::ReturnAllMemoryBackend;
    use crate::datasets::{
        LongMemEvalAnswer, LongMemEvalEntry, LongMemEvalMessage, LongMemEvalRole,
        adapt_longmemeval_entry,
    };
    use crate::judge::LongMemEvalJudge;
    use crate::model::{
        BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, GoldAnswerVariant, RetrievalBudget,
    };
    use crate::prompt::{
        DefaultPromptBuilder, LongMemEvalAnswerPromptProfile, LongMemEvalPromptConfig,
    };

    #[tokio::test]
    async fn pipeline_runs_end_to_end() {
        let entry = LongMemEvalEntry {
            question_id: "q1_abs".to_string(),
            question_type: "multi-session".to_string(),
            question: "Where?".to_string(),
            question_date: "2024-01-03".to_string(),
            answer: LongMemEvalAnswer::Text("answer".to_string()),
            answer_session_ids: vec!["s1".to_string()],
            haystack_dates: vec!["2024-01-01".to_string()],
            haystack_session_ids: vec!["s1".to_string()],
            haystack_sessions: vec![vec![LongMemEvalMessage {
                role: LongMemEvalRole::User,
                content: "hello".to_string(),
                has_answer: Some(true),
            }]],
        };
        let case = adapt_longmemeval_entry(&entry).unwrap();
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = DefaultPromptBuilder;
        let answerer = DebugAnswerer::new("unknown");
        let judge = LongMemEvalJudge;

        let mut pipeline = EvaluatePipeline {
            backend: &mut backend,
            prompt_builder: &prompt_builder,
            answerer: &answerer,
            judge: &judge,
            budget: RetrievalBudget {
                max_items: Some(10),
                max_tokens: None,
            },
            prompt_config: crate::config::PromptConfig {
                longmemeval: Some(LongMemEvalPromptConfig {
                    answer_profile: LongMemEvalAnswerPromptProfile::HistoryChats,
                    cot: false,
                }),
                locomo_kioku: None,
            },
        };

        let result = pipeline.run(&[case]).await.unwrap();

        assert_eq!(result.answers.len(), 1);
        assert!(result.answers[0].is_correct);
        assert_eq!(result.retrievals[0].retrieved_count, 1);
    }

    #[tokio::test]
    async fn pipeline_rejects_empty_cases() {
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = DefaultPromptBuilder;
        let answerer = DebugAnswerer::default();
        let judge = LongMemEvalJudge;

        let mut pipeline = EvaluatePipeline {
            backend: &mut backend,
            prompt_builder: &prompt_builder,
            answerer: &answerer,
            judge: &judge,
            budget: RetrievalBudget::default(),
            prompt_config: crate::config::PromptConfig {
                longmemeval: Some(LongMemEvalPromptConfig {
                    answer_profile: LongMemEvalAnswerPromptProfile::HistoryChats,
                    cot: false,
                }),
                locomo_kioku: None,
            },
        };

        let error = pipeline.run(&[]).await.unwrap_err().to_string();
        assert!(error.contains("requires at least one case"));
    }

    #[tokio::test]
    async fn pipeline_rejects_mixed_datasets() {
        let longmemeval_entry = LongMemEvalEntry {
            question_id: "q1".to_string(),
            question_type: "multi-session".to_string(),
            question: "Where?".to_string(),
            question_date: "2024-01-03".to_string(),
            answer: LongMemEvalAnswer::Text("answer".to_string()),
            answer_session_ids: vec!["s1".to_string()],
            haystack_dates: vec!["2024-01-01".to_string()],
            haystack_session_ids: vec!["s1".to_string()],
            haystack_sessions: vec![vec![LongMemEvalMessage {
                role: LongMemEvalRole::User,
                content: "hello".to_string(),
                has_answer: Some(true),
            }]],
        };
        let longmemeval_case = adapt_longmemeval_entry(&longmemeval_entry).unwrap();
        let locomo_case = BenchmarkCase {
            dataset: BenchmarkDataset::LoCoMo,
            case_id: "locomo:sample-1".to_string(),
            events: Vec::new(),
            questions: vec![BenchmarkQuestion {
                question_id: "locomo:sample-1:q0".to_string(),
                question: "Where?".to_string(),
                question_timestamp: None,
                gold_answers: vec!["answer".to_string()],
                evidence_event_ids: Vec::new(),
                evidence_session_ids: Vec::new(),
                category: Some(1),
                question_type: None,
                gold_answer_variant: GoldAnswerVariant::Default,
                is_abstention: false,
                metadata: serde_json::Value::Null,
            }],
            metadata: serde_json::Value::Null,
        };

        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = DefaultPromptBuilder;
        let answerer = DebugAnswerer::default();
        let judge = LongMemEvalJudge;

        let mut pipeline = EvaluatePipeline {
            backend: &mut backend,
            prompt_builder: &prompt_builder,
            answerer: &answerer,
            judge: &judge,
            budget: RetrievalBudget::default(),
            prompt_config: crate::config::PromptConfig {
                longmemeval: Some(LongMemEvalPromptConfig {
                    answer_profile: LongMemEvalAnswerPromptProfile::HistoryChats,
                    cot: false,
                }),
                locomo_kioku: None,
            },
        };

        let error = pipeline
            .run(&[longmemeval_case, locomo_case])
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("all cases to use the same dataset"));
    }
}
