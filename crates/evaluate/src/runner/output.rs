use anyhow::Context;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::config::ResolvedRunMetadata;

use super::EvaluatePipelineResult;

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
    use super::write_outputs;
    use std::fs::File;

    use crate::config::{
        AnswererKind, BackendKind, JudgeKind, ResolvedAnswererMetadata, ResolvedBackendMetadata,
        ResolvedJudgeMetadata, ResolvedPromptMetadata, ResolvedRunMetadata,
    };
    use crate::model::{
        AnswerLogRecord, BenchmarkDataset, DatasetMetrics, MetricProvenance, MetricsReport,
        RetrievalBudget, RetrievalLogRecord,
    };
    use crate::prompt::{LocomoKiokuPromptConfig, LongMemEvalKiokuPromptConfig};
    use crate::runner::EvaluatePipelineResult;

    #[test]
    fn write_outputs_preserves_locomo_kioku_schema_without_legacy_metrics_fields() {
        let temp_dir =
            std::env::temp_dir().join(format!("kioku-evaluate-locomo-test-{}", std::process::id()));
        if temp_dir.exists() {
            std::fs::remove_dir_all(&temp_dir).unwrap();
        }

        let result = EvaluatePipelineResult {
            answers: vec![AnswerLogRecord {
                dataset: BenchmarkDataset::LoCoMo,
                case_id: "case-1".to_string(),
                question_id: "q1".to_string(),
                question: "When was the meeting?".to_string(),
                generated_answer: "May 2019".to_string(),
                gold_answers: vec!["May 2019".to_string()],
                is_correct: true,
                score: 1.0,
                label: "CORRECT".to_string(),
                question_type: None,
                category: Some(2),
                is_abstention: false,
                answer_metadata: serde_json::json!({
                    "template_id": "locomo.kioku.answer.v1",
                    "answerer_model": "answerer-model",
                    "prompt": {
                        "template_id": "locomo.kioku.answer.v1",
                        "context_kind": "StructuredFacts"
                    },
                    "answerer": {
                        "kind": "openai-compatible"
                    },
                    "llm": {
                        "model_name": "answerer-model",
                        "finish_reason": "stop",
                        "latency_ms": 12
                    }
                }),
                judgement_metadata: serde_json::json!({
                    "judge_kind": "locomo_kioku_answer_llm",
                    "judge_model": "judge-model",
                    "judge_prompt_id": "locomo.kioku.judge.answer.v1",
                    "reason": "stub",
                }),
            }],
            retrievals: vec![RetrievalLogRecord {
                dataset: BenchmarkDataset::LoCoMo,
                case_id: "case-1".to_string(),
                question_id: "q1".to_string(),
                category: Some(2),
                retrieved_count: 2,
                retrieved_memory_ids: vec!["m1".to_string(), "m2".to_string()],
                retrieved_source_event_ids: vec!["e1".to_string()],
                context_kind: Some("structured-facts".to_string()),
                context_text: Some("1. [fact] The meeting happened in May 2019.".to_string()),
                is_sufficient: Some(true),
                score: Some(1.0),
                label: Some("SUFFICIENT".to_string()),
                judge_metadata: serde_json::json!({
                    "judge_kind": "locomo_kioku_retrieval_llm",
                    "judge_model": "judge-model",
                    "judge_prompt_id": "locomo.kioku.judge.retrieval.v1",
                    "supported_answer": "May 2019",
                    "reason": "stub",
                }),
                evidence_event_ids: vec!["e1".to_string()],
                evidence_session_ids: Vec::new(),
                metadata: serde_json::json!({
                    "backend_kind": "return_all",
                }),
            }],
            metrics: MetricsReport {
                dataset: BenchmarkDataset::LoCoMo,
                protocol: Some("locomo_kioku_v1".to_string()),
                provenance: MetricProvenance {
                    answer_judge_kind: Some("locomo_kioku_answer_llm".to_string()),
                    retrieval_judge_kind: Some("locomo_kioku_retrieval_llm".to_string()),
                    judge_kind: None,
                    metric_semantics_version: "locomo_kioku_v1".to_string(),
                    provisional: false,
                    locomo_overall_scope: Some("category_1_4".to_string()),
                    answer_judge_model: Some("judge-model".to_string()),
                    retrieval_judge_model: Some("judge-model".to_string()),
                    answer_judge_prompt_id: Some("locomo.kioku.judge.answer.v1".to_string()),
                    retrieval_judge_prompt_id: Some("locomo.kioku.judge.retrieval.v1".to_string()),
                    answerer_model: Some("answerer-model".to_string()),
                    context_tokenizer: None,
                },
                metrics: DatasetMetrics {
                    question_count: 1,
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
                    average_retrieved_item_count: 2.0,
                    average_context_token_count: None,
                    per_category_accuracy: Default::default(),
                    per_category_answer_accuracy: std::iter::once((
                        "2".to_string(),
                        crate::model::CategoryMetrics {
                            correct: 1,
                            total: 1,
                            accuracy: 1.0,
                        },
                    ))
                    .collect(),
                    per_category_retrieval_sufficiency_accuracy: std::iter::once((
                        "2".to_string(),
                        crate::model::CategoryMetrics {
                            correct: 1,
                            total: 1,
                            accuracy: 1.0,
                        },
                    ))
                    .collect(),
                    per_type_accuracy: Default::default(),
                    per_type_answer_accuracy: Default::default(),
                    per_type_retrieval_sufficiency_accuracy: Default::default(),
                },
            },
        };
        let resolved_run = ResolvedRunMetadata {
            evaluate_version: env!("CARGO_PKG_VERSION"),
            dataset: BenchmarkDataset::LoCoMo,
            input: temp_dir.join("input.json"),
            output_dir: temp_dir.clone(),
            backend: ResolvedBackendMetadata {
                kind: BackendKind::ReturnAll,
            },
            answerer: ResolvedAnswererMetadata {
                kind: AnswererKind::Debug,
                openai_compatible: None,
            },
            judge: Some(ResolvedJudgeMetadata {
                kind: JudgeKind::OpenAiCompatible,
                openai_compatible: None,
            }),
            retrieval: RetrievalBudget {
                max_items: Some(10),
                max_tokens: None,
            },
            prompt: ResolvedPromptMetadata {
                longmemeval_kioku: None,
                locomo_kioku: Some(LocomoKiokuPromptConfig {
                    answer_template_id: "locomo.kioku.answer.v1".to_string(),
                    answer_judge_prompt_id: "locomo.kioku.judge.answer.v1".to_string(),
                    retrieval_judge_prompt_id: "locomo.kioku.judge.retrieval.v1".to_string(),
                }),
            },
            context_tokenizer: None,
        };

        write_outputs(
            &temp_dir,
            &result,
            br#"[run]
dataset = "locomo"
[answerer]
kind = "debug"
"#,
            &resolved_run,
        )
        .unwrap();

        let answer_line = std::fs::read_to_string(temp_dir.join("answers.jsonl")).unwrap();
        let answer_record: serde_json::Value =
            serde_json::from_str(answer_line.lines().next().unwrap()).unwrap();
        let retrieval_line = std::fs::read_to_string(temp_dir.join("retrieval.jsonl")).unwrap();
        let retrieval_record: serde_json::Value =
            serde_json::from_str(retrieval_line.lines().next().unwrap()).unwrap();
        let metrics: serde_json::Value =
            serde_json::from_reader(File::open(temp_dir.join("metrics.json")).unwrap()).unwrap();

        assert_eq!(retrieval_record["context_kind"], "structured-facts");
        assert_eq!(retrieval_record["is_sufficient"], true);
        assert_eq!(
            retrieval_record["judge_metadata"]["judge_kind"],
            "locomo_kioku_retrieval_llm"
        );
        assert_eq!(metrics["protocol"], "locomo_kioku_v1");
        assert_eq!(metrics["metrics"]["overall_answer_accuracy"], 1.0);
        assert_eq!(
            metrics["metrics"]["overall_retrieval_sufficiency_accuracy"],
            1.0
        );
        assert!(metrics["metrics"].get("overall_accuracy").is_none());
        assert!(metrics["metrics"].get("per_category_accuracy").is_none());
        assert!(metrics["metrics"].get("per_type_accuracy").is_none());
        assert_eq!(
            answer_record["answer_metadata"]["prompt"]["context_kind"],
            "StructuredFacts"
        );
        assert_eq!(
            answer_record["answer_metadata"]["answerer"]["kind"],
            "openai-compatible"
        );
        assert_eq!(
            answer_record["answer_metadata"]["llm"]["model_name"],
            "answerer-model"
        );
        assert!(
            answer_record["answer_metadata"]["llm"]
                .get("raw_response")
                .is_none()
        );

        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn write_outputs_preserves_longmemeval_kioku_artifact_schema() {
        let temp_dir = std::env::temp_dir().join(format!(
            "kioku-evaluate-longmemeval-kioku-test-{}",
            std::process::id()
        ));
        if temp_dir.exists() {
            std::fs::remove_dir_all(&temp_dir).unwrap();
        }

        let result = EvaluatePipelineResult {
            answers: vec![AnswerLogRecord {
                dataset: BenchmarkDataset::LongMemEval,
                case_id: "case-1".to_string(),
                question_id: "q1".to_string(),
                question: "Where does the user live now?".to_string(),
                generated_answer: "Kyoto".to_string(),
                gold_answers: vec!["Kyoto".to_string()],
                is_correct: true,
                score: 1.0,
                label: "CORRECT".to_string(),
                question_type: Some("knowledge-update".to_string()),
                category: None,
                is_abstention: false,
                answer_metadata: serde_json::json!({
                    "template_id": "longmemeval.kioku.answer.v1",
                    "answerer_model": "answerer-model",
                    "prompt": {
                        "template_id": "longmemeval.kioku.answer.v1",
                        "context_kind": "HistoryChats",
                        "protocol": "longmemeval_kioku_v1"
                    },
                    "answerer": {
                        "kind": "openai-compatible"
                    },
                    "llm": {
                        "model_name": "answerer-model",
                        "finish_reason": "stop",
                        "latency_ms": 12
                    }
                }),
                judgement_metadata: serde_json::json!({
                    "judge_kind": "longmemeval_kioku_answer_llm",
                    "judge_model": "judge-model",
                    "judge_prompt_id": "longmemeval.kioku.judge.answer.v1",
                    "reason": "stub",
                }),
            }],
            retrievals: vec![RetrievalLogRecord {
                dataset: BenchmarkDataset::LongMemEval,
                case_id: "case-1".to_string(),
                question_id: "q1".to_string(),
                category: None,
                retrieved_count: 2,
                retrieved_memory_ids: vec!["m1".to_string(), "m2".to_string()],
                retrieved_source_event_ids: vec!["e1".to_string()],
                context_kind: Some("history-chats".to_string()),
                context_text: Some(
                    "### Session 1:\nSession Content:\nuser: I moved to Kyoto.".to_string(),
                ),
                is_sufficient: Some(true),
                score: Some(1.0),
                label: Some("SUFFICIENT".to_string()),
                judge_metadata: serde_json::json!({
                    "judge_kind": "longmemeval_kioku_retrieval_llm",
                    "judge_model": "judge-model",
                    "judge_prompt_id": "longmemeval.kioku.judge.retrieval.v1",
                    "supported_answer": "Kyoto",
                    "reason": "stub",
                }),
                evidence_event_ids: vec!["e1".to_string()],
                evidence_session_ids: vec!["s1".to_string()],
                metadata: serde_json::json!({
                    "backend_kind": "return_all",
                }),
            }],
            metrics: MetricsReport {
                dataset: BenchmarkDataset::LongMemEval,
                protocol: Some("longmemeval_kioku_v1".to_string()),
                provenance: MetricProvenance {
                    answer_judge_kind: Some("longmemeval_kioku_answer_llm".to_string()),
                    retrieval_judge_kind: Some("longmemeval_kioku_retrieval_llm".to_string()),
                    judge_kind: None,
                    metric_semantics_version: "longmemeval_kioku_v1".to_string(),
                    provisional: false,
                    locomo_overall_scope: None,
                    answer_judge_model: Some("judge-model".to_string()),
                    retrieval_judge_model: Some("judge-model".to_string()),
                    answer_judge_prompt_id: Some("longmemeval.kioku.judge.answer.v1".to_string()),
                    retrieval_judge_prompt_id: Some(
                        "longmemeval.kioku.judge.retrieval.v1".to_string(),
                    ),
                    answerer_model: Some("answerer-model".to_string()),
                    context_tokenizer: Some("whitespace_v1".to_string()),
                },
                metrics: DatasetMetrics {
                    question_count: 1,
                    non_abstention_question_count: Some(1),
                    abstention_question_count: Some(0),
                    scored_question_count: None,
                    overall_accuracy: None,
                    overall_answer_accuracy: Some(1.0),
                    overall_retrieval_sufficiency_accuracy: Some(1.0),
                    task_averaged_answer_accuracy: Some(1.0),
                    task_averaged_retrieval_sufficiency_accuracy: Some(1.0),
                    adversarial_accuracy: None,
                    abstention_accuracy: None,
                    abstention_answer_accuracy: None,
                    average_retrieved_item_count: 2.0,
                    average_context_token_count: Some(7.0),
                    per_category_accuracy: Default::default(),
                    per_category_answer_accuracy: Default::default(),
                    per_category_retrieval_sufficiency_accuracy: Default::default(),
                    per_type_accuracy: Default::default(),
                    per_type_answer_accuracy: std::iter::once((
                        "knowledge-update".to_string(),
                        crate::model::CategoryMetrics {
                            correct: 1,
                            total: 1,
                            accuracy: 1.0,
                        },
                    ))
                    .collect(),
                    per_type_retrieval_sufficiency_accuracy: std::iter::once((
                        "knowledge-update".to_string(),
                        crate::model::CategoryMetrics {
                            correct: 1,
                            total: 1,
                            accuracy: 1.0,
                        },
                    ))
                    .collect(),
                },
            },
        };
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
            judge: Some(ResolvedJudgeMetadata {
                kind: JudgeKind::OpenAiCompatible,
                openai_compatible: None,
            }),
            retrieval: RetrievalBudget {
                max_items: Some(10),
                max_tokens: None,
            },
            prompt: ResolvedPromptMetadata {
                longmemeval_kioku: Some(LongMemEvalKiokuPromptConfig {
                    answer_template_id: "longmemeval.kioku.answer.v1".to_string(),
                    answer_judge_prompt_id: "longmemeval.kioku.judge.answer.v1".to_string(),
                    retrieval_judge_prompt_id: "longmemeval.kioku.judge.retrieval.v1".to_string(),
                }),
                locomo_kioku: None,
            },
            context_tokenizer: Some("whitespace_v1".to_string()),
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

        let answer_line = std::fs::read_to_string(temp_dir.join("answers.jsonl")).unwrap();
        let answer_record: serde_json::Value =
            serde_json::from_str(answer_line.lines().next().unwrap()).unwrap();
        let retrieval_line = std::fs::read_to_string(temp_dir.join("retrieval.jsonl")).unwrap();
        let retrieval_record: serde_json::Value =
            serde_json::from_str(retrieval_line.lines().next().unwrap()).unwrap();
        let metrics: serde_json::Value =
            serde_json::from_reader(File::open(temp_dir.join("metrics.json")).unwrap()).unwrap();
        let resolved: serde_json::Value =
            serde_json::from_reader(File::open(temp_dir.join("run.resolved.json")).unwrap())
                .unwrap();

        assert_eq!(
            answer_record["answer_metadata"]["template_id"],
            "longmemeval.kioku.answer.v1"
        );
        assert_eq!(
            answer_record["answer_metadata"]["prompt"]["protocol"],
            "longmemeval_kioku_v1"
        );
        assert_eq!(
            answer_record["answer_metadata"]["answerer"]["kind"],
            "openai-compatible"
        );
        assert_eq!(
            answer_record["answer_metadata"]["llm"]["model_name"],
            "answerer-model"
        );
        assert!(
            answer_record["answer_metadata"]["llm"]
                .get("raw_response")
                .is_none()
        );
        assert_eq!(
            answer_record["judgement_metadata"]["judge_kind"],
            "longmemeval_kioku_answer_llm"
        );
        assert_eq!(retrieval_record["context_kind"], "history-chats");
        assert_eq!(retrieval_record["is_sufficient"], true);
        assert_eq!(
            retrieval_record["judge_metadata"]["judge_prompt_id"],
            "longmemeval.kioku.judge.retrieval.v1"
        );
        assert_eq!(metrics["protocol"], "longmemeval_kioku_v1");
        assert_eq!(metrics["metrics"]["overall_answer_accuracy"], 1.0);
        assert_eq!(
            metrics["metrics"]["overall_retrieval_sufficiency_accuracy"],
            1.0
        );
        assert_eq!(metrics["metrics"]["non_abstention_question_count"], 1);
        assert_eq!(metrics["metrics"]["abstention_question_count"], 0);
        assert_eq!(metrics["metrics"]["average_context_token_count"], 7.0);
        assert!(metrics["metrics"].get("overall_accuracy").is_none());
        assert_eq!(
            metrics["metrics"]["per_type_answer_accuracy"]["knowledge-update"]["accuracy"],
            1.0
        );
        assert_eq!(
            metrics["metrics"]["per_type_retrieval_sufficiency_accuracy"]["knowledge-update"]["accuracy"],
            1.0
        );
        assert_eq!(
            resolved["prompt"]["longmemeval_kioku"]["answer_template_id"],
            "longmemeval.kioku.answer.v1"
        );
        assert_eq!(resolved["context_tokenizer"], "whitespace_v1");

        std::fs::remove_dir_all(temp_dir).unwrap();
    }
}
