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
    use crate::config::{
        AnswererKind, BackendKind, ResolvedAnswererMetadata, ResolvedBackendMetadata,
        ResolvedPromptMetadata, ResolvedRunMetadata,
    };
    use crate::model::{
        AnswerLogRecord, BenchmarkDataset, DatasetMetrics, MetricProvenance, MetricsReport,
        RetrievalBudget, RetrievalLogRecord,
    };
    use crate::prompt::{LongMemEvalAnswerPromptProfile, LongMemEvalPromptConfig};
    use crate::runner::EvaluatePipelineResult;

    #[test]
    fn write_outputs_persists_all_expected_artifacts() {
        let temp_dir =
            std::env::temp_dir().join(format!("kioku-evaluate-test-{}", std::process::id()));
        if temp_dir.exists() {
            std::fs::remove_dir_all(&temp_dir).unwrap();
        }

        let result = EvaluatePipelineResult {
            answers: vec![AnswerLogRecord {
                dataset: BenchmarkDataset::LongMemEval,
                case_id: "case-1".to_string(),
                question_id: "q1".to_string(),
                question: "Where?".to_string(),
                generated_answer: "answer".to_string(),
                gold_answers: vec!["answer".to_string()],
                is_correct: true,
                score: 1.0,
                label: "correct".to_string(),
                question_type: Some("multi-session".to_string()),
                category: None,
                is_abstention: false,
                answer_metadata: serde_json::json!({
                    "prompt": {
                        "template_id": "longmemeval.answer.history_chats.v1"
                    }
                }),
                judgement_metadata: serde_json::Value::Null,
            }],
            retrievals: vec![RetrievalLogRecord {
                dataset: BenchmarkDataset::LongMemEval,
                case_id: "case-1".to_string(),
                question_id: "q1".to_string(),
                retrieved_count: 1,
                retrieved_event_ids: vec!["event-1".to_string()],
                evidence_event_ids: Vec::new(),
                evidence_session_ids: vec!["s1".to_string()],
                metadata: serde_json::Value::Null,
            }],
            metrics: MetricsReport {
                dataset: BenchmarkDataset::LongMemEval,
                provenance: MetricProvenance {
                    judge_kind: "longmemeval_exact_match".to_string(),
                    metric_semantics_version: "phase1-minimal-v1".to_string(),
                    provisional: true,
                    locomo_overall_scope: None,
                },
                metrics: DatasetMetrics {
                    question_count: 1,
                    scored_question_count: 1,
                    overall_accuracy: 1.0,
                    adversarial_accuracy: None,
                    abstention_accuracy: None,
                    average_retrieved_item_count: 1.0,
                    per_category_accuracy: Default::default(),
                    per_type_accuracy: Default::default(),
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
            retrieval: RetrievalBudget {
                max_items: Some(10),
                max_tokens: None,
            },
            prompt: ResolvedPromptMetadata {
                longmemeval: Some(LongMemEvalPromptConfig {
                    answer_profile: LongMemEvalAnswerPromptProfile::HistoryChats,
                    cot: false,
                }),
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
        assert_eq!(
            answer_record["answer_metadata"]["prompt"]["template_id"],
            "longmemeval.answer.history_chats.v1"
        );

        std::fs::remove_dir_all(temp_dir).unwrap();
    }
}
