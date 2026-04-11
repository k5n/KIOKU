use anyhow::{Context, ensure};

use crate::answerer::Answerer;
use crate::backend::MemoryBackend;
use crate::judge::{AnswerJudge, RetrievalJudge};
use crate::model::{
    AnswerLogRecord, AnswerRequest, BenchmarkCase, EvalScope, QueryInput, RetrievalBudget,
    RetrievalLogRecord,
};
use crate::prompt::{PromptBuildRequest, PromptBuilder};
use crate::token_counter::TokenCounter;

use super::helpers::{
    context_kind_name, extract_answerer_model, merge_metadata, sanitize_answer_metadata,
};
use super::{
    ContextTokenPolicy, DatasetEvaluationProtocol, EvaluatePipelineResult, EvaluatedQuestion,
};

pub(crate) struct CommonEvaluatePipeline<
    'a,
    B: ?Sized,
    P: ?Sized,
    A: ?Sized,
    AJ: ?Sized,
    RJ: ?Sized,
    Protocol,
> {
    pub backend: &'a mut B,
    pub prompt_builder: &'a P,
    pub answerer: &'a A,
    pub answer_judge: &'a AJ,
    pub retrieval_judge: &'a RJ,
    pub token_counter: Option<&'a dyn TokenCounter>,
    pub budget: RetrievalBudget,
    pub protocol: &'a Protocol,
}

impl<'a, B, P, A, AJ, RJ, Protocol> CommonEvaluatePipeline<'a, B, P, A, AJ, RJ, Protocol>
where
    B: MemoryBackend + ?Sized,
    P: PromptBuilder + ?Sized,
    A: Answerer + ?Sized,
    AJ: AnswerJudge + ?Sized,
    RJ: RetrievalJudge + ?Sized,
    Protocol: DatasetEvaluationProtocol,
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
        ensure!(
            dataset == self.protocol.dataset(),
            "evaluate pipeline dataset mismatch: cases use `{}` but protocol expects `{}`",
            dataset.as_str(),
            self.protocol.dataset().as_str(),
        );

        let required_token_counter = match self.protocol.context_token_policy() {
            ContextTokenPolicy::Required => Some(self.token_counter.context(
                "evaluate pipeline requires token_counter when protocol context_token_policy is Required",
            )?),
            ContextTokenPolicy::Optional => None,
        };
        let context_tokenizer = required_token_counter.map(TokenCounter::name);

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

            for question in case
                .questions
                .iter()
                .filter(|question| self.protocol.include_question(question))
            {
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
                            case,
                            question,
                            prompt_context,
                            profile: self.protocol.answer_prompt_profile(),
                        })?;
                let generated_answer = self
                    .answerer
                    .answer(AnswerRequest {
                        prompt: &prepared_prompt,
                    })
                    .await?;
                let answer_judgement = self
                    .answer_judge
                    .judge_answer(question, &generated_answer)
                    .await?;
                let answerer_model = extract_answerer_model(&generated_answer.metadata);
                let context_token_count = required_token_counter
                    .map(|token_counter| token_counter.count_text_tokens(&prompt_context.text))
                    .transpose()?;

                let evaluated = EvaluatedQuestion {
                    dataset,
                    case,
                    question,
                    prompt_context: query_output.prompt_context,
                    query_metadata: query_output.metadata,
                    retrieval_judgement,
                    prepared_prompt,
                    generated_answer,
                    answer_judgement,
                    answerer_model,
                    context_token_count,
                };

                retrievals.push(build_retrieval_log(&evaluated));
                answers.push(build_answer_log(&evaluated));
                metric_inputs.push(self.protocol.build_metric_input(&evaluated)?);
            }
        }

        let metrics = self
            .protocol
            .build_metrics(&metric_inputs, context_tokenizer)?;

        Ok(EvaluatePipelineResult {
            answers,
            retrievals,
            metrics,
        })
    }
}

fn build_retrieval_log(evaluated: &EvaluatedQuestion<'_>) -> RetrievalLogRecord {
    RetrievalLogRecord {
        dataset: evaluated.dataset,
        case_id: evaluated.case.case_id.clone(),
        question_id: evaluated.question.question_id.clone(),
        category: evaluated.question.category,
        context_kind: Some(context_kind_name(&evaluated.prompt_context)),
        context_text: Some(evaluated.prompt_context.text.clone()),
        is_sufficient: Some(evaluated.retrieval_judgement.passed),
        score: Some(evaluated.retrieval_judgement.score),
        label: Some(evaluated.retrieval_judgement.label.clone()),
        judge_metadata: evaluated.retrieval_judgement.metadata.clone(),
        evidence_event_ids: evaluated.question.evidence_event_ids.clone(),
        evidence_session_ids: evaluated.question.evidence_session_ids.clone(),
        metadata: merge_metadata(
            &evaluated.query_metadata,
            &evaluated.prompt_context.metadata,
        ),
    }
}

fn build_answer_log(evaluated: &EvaluatedQuestion<'_>) -> AnswerLogRecord {
    AnswerLogRecord {
        dataset: evaluated.dataset,
        case_id: evaluated.case.case_id.clone(),
        question_id: evaluated.question.question_id.clone(),
        question: evaluated.question.question.clone(),
        generated_answer: evaluated.generated_answer.text.clone(),
        gold_answers: evaluated.question.gold_answers.clone(),
        is_correct: evaluated.answer_judgement.passed,
        score: evaluated.answer_judgement.score,
        label: evaluated.answer_judgement.label.clone(),
        question_type: evaluated.question.question_type.clone(),
        category: evaluated.question.category,
        is_abstention: evaluated.question.is_abstention,
        answer_metadata: sanitize_answer_metadata(
            evaluated.generated_answer.metadata.clone(),
            &evaluated.prepared_prompt.template_id,
            &evaluated.answerer_model,
        ),
        judgement_metadata: evaluated.answer_judgement.metadata.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::CommonEvaluatePipeline;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    use crate::answerer::DebugAnswerer;
    use crate::backend::ReturnAllMemoryBackend;
    use crate::judge::{AnswerJudge, BinaryJudgement, RetrievalJudge};
    use crate::model::{
        BenchmarkCase, BenchmarkDataset, BenchmarkEvent, BenchmarkQuestion, CategoryMetrics,
        DatasetMetrics, GeneratedAnswer, GoldAnswerVariant, MetricProvenance, MetricsReport,
        RetrievalBudget,
    };
    use crate::prompt::{
        AnswerPromptProfile, DefaultPromptBuilder, LocomoKiokuPromptConfig, PromptContext,
    };
    use crate::runner::{ContextTokenPolicy, DatasetEvaluationProtocol, EvaluatedQuestion};
    use crate::token_counter::WhitespaceTokenCounter;

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
                    "judge_kind": "test_answer_judge",
                    "judge_model": "judge-model",
                }),
            })
        }
    }

    #[derive(Debug, Default)]
    struct RecordingRetrievalJudge;

    #[async_trait]
    impl RetrievalJudge for RecordingRetrievalJudge {
        async fn judge_retrieval(
            &self,
            _question: &BenchmarkQuestion,
            context: &PromptContext,
        ) -> anyhow::Result<BinaryJudgement> {
            Ok(BinaryJudgement {
                passed: !context.text.is_empty(),
                score: 1.0,
                label: "SUFFICIENT".to_string(),
                metadata: serde_json::json!({
                    "judge_kind": "test_retrieval_judge",
                    "judge_model": "judge-model",
                }),
            })
        }
    }

    #[derive(Debug)]
    struct TestProtocol<'a> {
        dataset: BenchmarkDataset,
        context_token_policy: ContextTokenPolicy,
        prompt: &'a LocomoKiokuPromptConfig,
        included_categories: Vec<u8>,
        seen_context_token_counts: Arc<Mutex<Vec<Option<usize>>>>,
    }

    impl DatasetEvaluationProtocol for TestProtocol<'_> {
        type MetricInput = String;

        fn dataset(&self) -> BenchmarkDataset {
            self.dataset
        }

        fn context_token_policy(&self) -> ContextTokenPolicy {
            self.context_token_policy
        }

        fn include_question(&self, question: &BenchmarkQuestion) -> bool {
            question
                .category
                .is_some_and(|category| self.included_categories.contains(&category))
        }

        fn answer_prompt_profile<'a>(&'a self) -> AnswerPromptProfile<'a> {
            AnswerPromptProfile::LoCoMoKioku(self.prompt)
        }

        fn build_metric_input(
            &self,
            evaluated: &EvaluatedQuestion<'_>,
        ) -> anyhow::Result<Self::MetricInput> {
            self.seen_context_token_counts
                .lock()
                .unwrap()
                .push(evaluated.context_token_count);
            Ok(evaluated.question.question_id.clone())
        }

        fn build_metrics(
            &self,
            inputs: &[Self::MetricInput],
            context_tokenizer: Option<&str>,
        ) -> anyhow::Result<MetricsReport> {
            Ok(MetricsReport {
                dataset: self.dataset,
                protocol: Some("test_protocol".to_string()),
                provenance: MetricProvenance {
                    answer_judge_kind: Some("test_answer_judge".to_string()),
                    retrieval_judge_kind: Some("test_retrieval_judge".to_string()),
                    judge_kind: None,
                    metric_semantics_version: "test_protocol".to_string(),
                    provisional: false,
                    locomo_overall_scope: None,
                    answer_judge_model: Some("judge-model".to_string()),
                    retrieval_judge_model: Some("judge-model".to_string()),
                    answer_judge_prompt_id: Some(self.prompt.answer_judge_prompt_id.clone()),
                    retrieval_judge_prompt_id: Some(self.prompt.retrieval_judge_prompt_id.clone()),
                    answerer_model: Some("debug".to_string()),
                    context_tokenizer: context_tokenizer.map(ToString::to_string),
                },
                metrics: DatasetMetrics {
                    question_count: inputs.len(),
                    non_abstention_question_count: None,
                    abstention_question_count: None,
                    scored_question_count: None,
                    overall_accuracy: None,
                    overall_answer_accuracy: Some(1.0),
                    overall_retrieval_sufficiency_accuracy: Some(1.0),
                    task_averaged_answer_accuracy: None,
                    task_averaged_retrieval_sufficiency_accuracy: None,
                    adversarial_accuracy: None,
                    abstention_accuracy: None,
                    abstention_answer_accuracy: None,
                    average_context_token_count: None,
                    per_category_accuracy:
                        std::collections::BTreeMap::<String, CategoryMetrics>::new(),
                    per_category_answer_accuracy: std::collections::BTreeMap::<
                        String,
                        CategoryMetrics,
                    >::new(),
                    per_category_retrieval_sufficiency_accuracy: std::collections::BTreeMap::<
                        String,
                        CategoryMetrics,
                    >::new(),
                    per_type_accuracy: std::collections::BTreeMap::<String, CategoryMetrics>::new(),
                    per_type_answer_accuracy:
                        std::collections::BTreeMap::<String, CategoryMetrics>::new(),
                    per_type_retrieval_sufficiency_accuracy: std::collections::BTreeMap::<
                        String,
                        CategoryMetrics,
                    >::new(),
                },
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

    fn sample_case(dataset: BenchmarkDataset) -> BenchmarkCase {
        BenchmarkCase {
            dataset,
            case_id: format!("{}:sample", dataset.as_str()),
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
                    category: Some(1),
                    question_type: Some("episodic".to_string()),
                    gold_answer_variant: GoldAnswerVariant::Default,
                    is_abstention: false,
                    metadata: serde_json::json!({
                        "raw_question_date": "2024-01-03",
                    }),
                },
                BenchmarkQuestion {
                    question_id: "q2".to_string(),
                    question: "Skipped?".to_string(),
                    question_timestamp: None,
                    gold_answers: vec!["skip".to_string()],
                    evidence_event_ids: Vec::new(),
                    evidence_session_ids: Vec::new(),
                    category: Some(5),
                    question_type: Some("episodic".to_string()),
                    gold_answer_variant: GoldAnswerVariant::Default,
                    is_abstention: false,
                    metadata: serde_json::json!({
                        "raw_question_date": "2024-01-03",
                    }),
                },
            ],
            metadata: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn common_runner_respects_protocol_question_filter() {
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = DefaultPromptBuilder;
        let answerer = DebugAnswerer::new("correct");
        let answer_judge = RecordingAnswerJudge;
        let retrieval_judge = RecordingRetrievalJudge;
        let prompt = sample_prompt_config();
        let protocol = TestProtocol {
            dataset: BenchmarkDataset::LoCoMo,
            context_token_policy: ContextTokenPolicy::Optional,
            prompt: &prompt,
            included_categories: vec![1],
            seen_context_token_counts: Arc::new(Mutex::new(Vec::new())),
        };
        let mut pipeline = CommonEvaluatePipeline {
            backend: &mut backend,
            prompt_builder: &prompt_builder,
            answerer: &answerer,
            answer_judge: &answer_judge,
            retrieval_judge: &retrieval_judge,
            token_counter: None,
            budget: RetrievalBudget::default(),
            protocol: &protocol,
        };

        let result = pipeline
            .run(&[sample_case(BenchmarkDataset::LoCoMo)])
            .await
            .unwrap();

        assert_eq!(result.answers.len(), 1);
        assert_eq!(result.retrievals.len(), 1);
        assert_eq!(result.metrics.metrics.question_count, 1);
    }

    #[tokio::test]
    async fn common_runner_allows_optional_policy_without_token_counter() {
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = DefaultPromptBuilder;
        let answerer = DebugAnswerer::new("correct");
        let answer_judge = RecordingAnswerJudge;
        let retrieval_judge = RecordingRetrievalJudge;
        let seen_context_token_counts = Arc::new(Mutex::new(Vec::new()));
        let prompt = sample_prompt_config();
        let protocol = TestProtocol {
            dataset: BenchmarkDataset::LoCoMo,
            context_token_policy: ContextTokenPolicy::Optional,
            prompt: &prompt,
            included_categories: vec![1],
            seen_context_token_counts: seen_context_token_counts.clone(),
        };
        let mut pipeline = CommonEvaluatePipeline {
            backend: &mut backend,
            prompt_builder: &prompt_builder,
            answerer: &answerer,
            answer_judge: &answer_judge,
            retrieval_judge: &retrieval_judge,
            token_counter: None,
            budget: RetrievalBudget::default(),
            protocol: &protocol,
        };

        let result = pipeline
            .run(&[sample_case(BenchmarkDataset::LoCoMo)])
            .await
            .unwrap();

        assert_eq!(result.metrics.provenance.context_tokenizer, None);
        assert_eq!(
            seen_context_token_counts.lock().unwrap().as_slice(),
            &[None]
        );
    }

    #[tokio::test]
    async fn common_runner_requires_token_counter_for_required_policy() {
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = DefaultPromptBuilder;
        let answerer = DebugAnswerer::new("correct");
        let answer_judge = RecordingAnswerJudge;
        let retrieval_judge = RecordingRetrievalJudge;
        let prompt = sample_prompt_config();
        let protocol = TestProtocol {
            dataset: BenchmarkDataset::LongMemEval,
            context_token_policy: ContextTokenPolicy::Required,
            prompt: &prompt,
            included_categories: vec![1],
            seen_context_token_counts: Arc::new(Mutex::new(Vec::new())),
        };
        let mut pipeline = CommonEvaluatePipeline {
            backend: &mut backend,
            prompt_builder: &prompt_builder,
            answerer: &answerer,
            answer_judge: &answer_judge,
            retrieval_judge: &retrieval_judge,
            token_counter: None,
            budget: RetrievalBudget::default(),
            protocol: &protocol,
        };

        let error = pipeline
            .run(&[sample_case(BenchmarkDataset::LongMemEval)])
            .await
            .unwrap_err();

        assert!(error.to_string().contains("token_counter"));
    }

    #[tokio::test]
    async fn common_runner_fails_on_dataset_protocol_mismatch() {
        let mut backend = ReturnAllMemoryBackend::default();
        let prompt_builder = DefaultPromptBuilder;
        let answerer = DebugAnswerer::new("correct");
        let answer_judge = RecordingAnswerJudge;
        let retrieval_judge = RecordingRetrievalJudge;
        let prompt = sample_prompt_config();
        let token_counter = WhitespaceTokenCounter;
        let protocol = TestProtocol {
            dataset: BenchmarkDataset::LongMemEval,
            context_token_policy: ContextTokenPolicy::Optional,
            prompt: &prompt,
            included_categories: vec![1],
            seen_context_token_counts: Arc::new(Mutex::new(Vec::new())),
        };
        let mut pipeline = CommonEvaluatePipeline {
            backend: &mut backend,
            prompt_builder: &prompt_builder,
            answerer: &answerer,
            answer_judge: &answer_judge,
            retrieval_judge: &retrieval_judge,
            token_counter: Some(&token_counter),
            budget: RetrievalBudget::default(),
            protocol: &protocol,
        };

        let error = pipeline
            .run(&[sample_case(BenchmarkDataset::LoCoMo)])
            .await
            .unwrap_err();

        assert!(error.to_string().contains("dataset mismatch"));
    }
}
