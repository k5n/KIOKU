use anyhow::{Context, ensure};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::answerer::Answerer;
use crate::backend::MemoryBackend;
use crate::config::ResolvedRunMetadata;
use crate::judge::{Judge, Judgement};
use crate::model::{
    AnswerLogRecord, AnswerRequest, BenchmarkCase, BenchmarkDataset, CategoryMetrics,
    DatasetMetrics, EvalScope, MetricProvenance, MetricsReport, QueryInput, RetrievalBudget,
    RetrievalLogRecord,
};

pub struct EvaluatePipeline<'a, B: ?Sized, A: ?Sized, J> {
    pub backend: &'a mut B,
    pub answerer: &'a A,
    pub judge: &'a J,
    pub budget: RetrievalBudget,
}

#[derive(Debug)]
pub struct EvaluatePipelineResult {
    pub answers: Vec<AnswerLogRecord>,
    pub retrievals: Vec<RetrievalLogRecord>,
    pub metrics: MetricsReport,
}

impl<'a, B, A, J> EvaluatePipeline<'a, B, A, J>
where
    B: MemoryBackend + ?Sized,
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
                        metadata: serde_json::Value::Null,
                    })
                    .await?;

                let generated = self
                    .answerer
                    .answer(AnswerRequest {
                        dataset,
                        case,
                        question,
                        retrieved: &query_output.retrieved,
                    })
                    .await?;

                let judgement = self.judge.judge(question, &generated).await?;
                retrieved_counts.push(query_output.retrieved.len() as f32);

                retrievals.push(RetrievalLogRecord {
                    dataset,
                    case_id: case.case_id.clone(),
                    question_id: question.question_id.clone(),
                    retrieved_count: query_output.retrieved.len(),
                    retrieved_event_ids: query_output
                        .retrieved
                        .iter()
                        .map(|memory| memory.event_id.clone())
                        .collect(),
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
                    is_correct: judgement.is_correct,
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

fn build_metrics(
    dataset: BenchmarkDataset,
    judgements: &[(&crate::model::BenchmarkQuestion, Judgement)],
    retrieved_counts: &[f32],
) -> MetricsReport {
    let applicable: Vec<_> = match dataset {
        BenchmarkDataset::LoCoMo => judgements
            .iter()
            .filter(|(question, _)| question.category != Some(5))
            .collect(),
        BenchmarkDataset::LongMemEval => judgements.iter().collect(),
    };
    let overall_correct = applicable
        .iter()
        .filter(|(_, judgement)| judgement.is_correct)
        .count();
    let overall_total = applicable.len();

    let mut per_category_accuracy = BTreeMap::new();
    let mut per_type_accuracy = BTreeMap::new();
    let mut adversarial_total = 0usize;
    let mut adversarial_correct = 0usize;
    let mut abstention_total = 0usize;
    let mut abstention_correct = 0usize;

    for (question, judgement) in judgements {
        if let Some(category) = question.category {
            let key = category.to_string();
            let entry = per_category_accuracy
                .entry(key)
                .or_insert_with(|| CategoryMetrics {
                    correct: 0,
                    total: 0,
                    accuracy: 0.0,
                });
            entry.total += 1;
            if judgement.is_correct {
                entry.correct += 1;
            }

            if category == 5 {
                adversarial_total += 1;
                if judgement.is_correct {
                    adversarial_correct += 1;
                }
            }
        }

        if let Some(question_type) = &question.question_type {
            let entry = per_type_accuracy
                .entry(question_type.clone())
                .or_insert_with(|| CategoryMetrics {
                    correct: 0,
                    total: 0,
                    accuracy: 0.0,
                });
            entry.total += 1;
            if judgement.is_correct {
                entry.correct += 1;
            }
        }

        if question.is_abstention {
            abstention_total += 1;
            if judgement.is_correct {
                abstention_correct += 1;
            }
        }
    }

    finalize_category_metrics(&mut per_category_accuracy);
    finalize_category_metrics(&mut per_type_accuracy);

    let average_retrieved_item_count = if retrieved_counts.is_empty() {
        0.0
    } else {
        retrieved_counts.iter().sum::<f32>() / retrieved_counts.len() as f32
    };

    MetricsReport {
        dataset,
        provenance: MetricProvenance {
            judge_kind: match dataset {
                BenchmarkDataset::LoCoMo => "locomo_exact_match".to_string(),
                BenchmarkDataset::LongMemEval => "longmemeval_exact_match".to_string(),
            },
            metric_semantics_version: "phase1-minimal-v1".to_string(),
            provisional: true,
            locomo_overall_scope: matches!(dataset, BenchmarkDataset::LoCoMo)
                .then(|| "category_1_4".to_string()),
        },
        metrics: DatasetMetrics {
            question_count: judgements.len(),
            scored_question_count: overall_total,
            overall_accuracy: ratio(overall_correct, overall_total),
            adversarial_accuracy: (adversarial_total > 0)
                .then(|| ratio(adversarial_correct, adversarial_total)),
            abstention_accuracy: (abstention_total > 0)
                .then(|| ratio(abstention_correct, abstention_total)),
            average_retrieved_item_count,
            per_category_accuracy,
            per_type_accuracy,
        },
    }
}

fn finalize_category_metrics(metrics: &mut BTreeMap<String, CategoryMetrics>) {
    for metric in metrics.values_mut() {
        metric.accuracy = ratio(metric.correct, metric.total);
    }
}

fn ratio(correct: usize, total: usize) -> f32 {
    if total == 0 {
        0.0
    } else {
        correct as f32 / total as f32
    }
}

pub fn write_outputs(
    output_dir: &Path,
    result: &EvaluatePipelineResult,
    raw_config: &[u8],
    resolved_run: &ResolvedRunMetadata,
) -> anyhow::Result<()> {
    std::fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "failed to create output directory `{}`",
            output_dir.display()
        )
    })?;
    std::fs::write(output_dir.join("run.config.toml"), raw_config).with_context(|| {
        format!(
            "failed to write `{}`",
            output_dir.join("run.config.toml").display()
        )
    })?;
    write_json(output_dir.join("run.resolved.json"), resolved_run)?;
    write_jsonl(output_dir.join("answers.jsonl"), &result.answers)?;
    write_jsonl(output_dir.join("retrieval.jsonl"), &result.retrievals)?;
    write_json(output_dir.join("metrics.json"), &result.metrics)?;
    Ok(())
}

fn write_jsonl<T: serde::Serialize>(path: impl AsRef<Path>, records: &[T]) -> anyhow::Result<()> {
    let file = File::create(path.as_ref())
        .with_context(|| format!("failed to create `{}`", path.as_ref().display()))?;
    let mut writer = BufWriter::new(file);
    for record in records {
        serde_json::to_writer(&mut writer, record)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(())
}

fn write_json<T: serde::Serialize>(path: impl AsRef<Path>, record: &T) -> anyhow::Result<()> {
    let file = File::create(path.as_ref())
        .with_context(|| format!("failed to create `{}`", path.as_ref().display()))?;
    serde_json::to_writer_pretty(BufWriter::new(file), record)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EvaluatePipeline, write_outputs};
    use crate::answerer::DebugAnswerer;
    use crate::backend::ReturnAllMemoryBackend;
    use crate::config::{
        AnswererKind, BackendKind, ResolvedAnswererMetadata, ResolvedBackendMetadata,
        ResolvedRunMetadata,
    };
    use crate::datasets::{
        LongMemEvalAnswer, LongMemEvalEntry, LongMemEvalMessage, LongMemEvalRole,
        adapt_longmemeval_entry,
    };
    use crate::judge::{LoCoMoJudge, LongMemEvalJudge};
    use crate::model::{
        BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, GoldAnswerVariant, RetrievalBudget,
    };

    #[tokio::test]
    async fn pipeline_runs_end_to_end_and_writes_outputs() {
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
        let answerer = DebugAnswerer::new("unknown");
        let judge = LongMemEvalJudge;

        let mut pipeline = EvaluatePipeline {
            backend: &mut backend,
            answerer: &answerer,
            judge: &judge,
            budget: RetrievalBudget {
                max_items: Some(10),
                max_tokens: None,
            },
        };

        let result = pipeline.run(&[case]).await.unwrap();

        assert_eq!(result.answers.len(), 1);
        assert!(result.answers[0].is_correct);
        assert_eq!(result.retrievals[0].retrieved_count, 1);

        let temp_dir =
            std::env::temp_dir().join(format!("kioku-evaluate-test-{}", std::process::id()));
        if temp_dir.exists() {
            std::fs::remove_dir_all(&temp_dir).unwrap();
        }
        let resolved_run = ResolvedRunMetadata {
            evaluate_version: env!("CARGO_PKG_VERSION"),
            dataset: BenchmarkDataset::LongMemEval,
            input: temp_dir.join("input.json"),
            output_dir: temp_dir.clone(),
            backend: ResolvedBackendMetadata {
                kind: BackendKind::ReturnAll,
            },
            answerer: ResolvedAnswererMetadata {
                kind: AnswererKind::Debug,
                openai_compatible: None,
            },
            retrieval: RetrievalBudget {
                max_items: Some(10),
                max_tokens: None,
            },
        };
        write_outputs(
            &temp_dir,
            &result,
            br#"[run]
dataset = "longmemeval"
[answerer]
kind = "debug"
"#,
            &resolved_run,
        )
        .unwrap();
        let saved_config = std::fs::read(temp_dir.join("run.config.toml")).unwrap();
        let answer_line = std::fs::read_to_string(temp_dir.join("answers.jsonl")).unwrap();
        let answer_record: serde_json::Value =
            serde_json::from_str(answer_line.lines().next().unwrap()).unwrap();
        assert!(temp_dir.join("answers.jsonl").exists());
        assert!(temp_dir.join("retrieval.jsonl").exists());
        assert!(temp_dir.join("metrics.json").exists());
        assert!(temp_dir.join("run.config.toml").exists());
        assert!(temp_dir.join("run.resolved.json").exists());
        assert_eq!(
            saved_config,
            br#"[run]
dataset = "longmemeval"
[answerer]
kind = "debug"
"#
        );
        assert!(answer_record["answer_metadata"]["dataset"].is_null());
        assert!(answer_record["answer_metadata"]["case_id"].is_null());
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[tokio::test]
    async fn pipeline_rejects_empty_cases() {
        let mut backend = ReturnAllMemoryBackend::default();
        let answerer = DebugAnswerer::default();
        let judge = LongMemEvalJudge;

        let mut pipeline = EvaluatePipeline {
            backend: &mut backend,
            answerer: &answerer,
            judge: &judge,
            budget: RetrievalBudget::default(),
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
        let answerer = DebugAnswerer::default();
        let judge = LongMemEvalJudge;

        let mut pipeline = EvaluatePipeline {
            backend: &mut backend,
            answerer: &answerer,
            judge: &judge,
            budget: RetrievalBudget::default(),
        };

        let error = pipeline
            .run(&[longmemeval_case, locomo_case])
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("all cases to use the same dataset"));
    }

    #[tokio::test]
    async fn locomo_metrics_separate_total_and_scored_question_counts() {
        let case = BenchmarkCase {
            dataset: BenchmarkDataset::LoCoMo,
            case_id: "locomo:sample-1".to_string(),
            events: Vec::new(),
            questions: vec![
                BenchmarkQuestion {
                    question_id: "locomo:sample-1:q0".to_string(),
                    question: "Q1".to_string(),
                    question_timestamp: None,
                    gold_answers: vec!["answer".to_string()],
                    evidence_event_ids: Vec::new(),
                    evidence_session_ids: Vec::new(),
                    category: Some(1),
                    question_type: None,
                    gold_answer_variant: GoldAnswerVariant::Default,
                    is_abstention: false,
                    metadata: serde_json::Value::Null,
                },
                BenchmarkQuestion {
                    question_id: "locomo:sample-1:q1".to_string(),
                    question: "Q2".to_string(),
                    question_timestamp: None,
                    gold_answers: vec!["different".to_string()],
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
        };
        let mut backend = ReturnAllMemoryBackend::default();
        let answerer = DebugAnswerer::new("answer");
        let judge = LoCoMoJudge;

        let mut pipeline = EvaluatePipeline {
            backend: &mut backend,
            answerer: &answerer,
            judge: &judge,
            budget: RetrievalBudget::default(),
        };

        let result = pipeline.run(&[case]).await.unwrap();

        assert_eq!(result.metrics.metrics.question_count, 2);
        assert_eq!(result.metrics.metrics.scored_question_count, 1);
        assert_eq!(result.metrics.metrics.overall_accuracy, 1.0);
        assert_eq!(result.metrics.metrics.adversarial_accuracy, Some(0.0));
    }
}
