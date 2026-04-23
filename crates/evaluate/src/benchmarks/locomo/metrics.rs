use std::collections::BTreeMap;

use crate::common::{
    judge::BinaryJudgement,
    model::{BenchmarkDataset, CategoryMetrics, DatasetMetrics, MetricProvenance, MetricsReport},
};

#[derive(Debug, Clone)]
pub(crate) struct LoCoMoKiokuMetricInput {
    pub(crate) category: u8,
    pub(crate) answer: BinaryJudgement,
    pub(crate) retrieval: BinaryJudgement,
    pub(crate) answerer_model: String,
}

pub(crate) fn build_metrics(
    inputs: &[LoCoMoKiokuMetricInput],
    answer_judge_prompt_id: &str,
    retrieval_judge_prompt_id: &str,
) -> MetricsReport {
    let mut per_category_answer_accuracy = BTreeMap::new();
    let mut per_category_retrieval_sufficiency_accuracy = BTreeMap::new();

    let mut answer_correct = 0usize;
    let mut retrieval_correct = 0usize;
    for input in inputs {
        let key = input.category.to_string();

        let answer_entry = per_category_answer_accuracy
            .entry(key.clone())
            .or_insert_with(empty_category_metrics);
        answer_entry.total += 1;
        if input.answer.passed {
            answer_entry.correct += 1;
            answer_correct += 1;
        }

        let retrieval_entry = per_category_retrieval_sufficiency_accuracy
            .entry(key)
            .or_insert_with(empty_category_metrics);
        retrieval_entry.total += 1;
        if input.retrieval.passed {
            retrieval_entry.correct += 1;
            retrieval_correct += 1;
        }
    }

    finalize_category_metrics(&mut per_category_answer_accuracy);
    finalize_category_metrics(&mut per_category_retrieval_sufficiency_accuracy);

    let answer_metadata = inputs.first().map(|input| &input.answer.metadata);
    let retrieval_metadata = inputs.first().map(|input| &input.retrieval.metadata);

    MetricsReport {
        dataset: BenchmarkDataset::LoCoMo,
        protocol: Some("locomo_kioku_v1".to_string()),
        provenance: MetricProvenance {
            answer_judge_kind: Some("locomo_kioku_answer_llm".to_string()),
            retrieval_judge_kind: Some("locomo_kioku_retrieval_llm".to_string()),
            judge_kind: None,
            metric_semantics_version: "locomo_kioku_v1".to_string(),
            provisional: false,
            locomo_overall_scope: Some("category_1_4".to_string()),
            answer_judge_model: answer_metadata
                .and_then(|metadata| metadata.get("judge_model"))
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
            retrieval_judge_model: retrieval_metadata
                .and_then(|metadata| metadata.get("judge_model"))
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string),
            answer_judge_prompt_id: Some(answer_judge_prompt_id.to_string()),
            retrieval_judge_prompt_id: Some(retrieval_judge_prompt_id.to_string()),
            answerer_model: inputs.first().map(|input| input.answerer_model.clone()),
            context_tokenizer: None,
        },
        metrics: DatasetMetrics {
            question_count: inputs.len(),
            non_abstention_question_count: None,
            abstention_question_count: None,
            scored_question_count: None,
            overall_accuracy: None,
            overall_answer_accuracy: Some(ratio(answer_correct, inputs.len())),
            overall_retrieval_sufficiency_accuracy: Some(ratio(retrieval_correct, inputs.len())),
            task_averaged_answer_accuracy: None,
            task_averaged_retrieval_sufficiency_accuracy: None,
            adversarial_accuracy: None,
            abstention_accuracy: None,
            abstention_answer_accuracy: None,
            average_context_token_count: None,
            per_category_accuracy: BTreeMap::new(),
            per_category_answer_accuracy,
            per_category_retrieval_sufficiency_accuracy,
            per_type_accuracy: BTreeMap::new(),
            per_type_answer_accuracy: BTreeMap::new(),
            per_type_retrieval_sufficiency_accuracy: BTreeMap::new(),
        },
    }
}

fn empty_category_metrics() -> CategoryMetrics {
    CategoryMetrics {
        correct: 0,
        total: 0,
        accuracy: 0.0,
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

#[cfg(test)]
mod tests {
    use super::{LoCoMoKiokuMetricInput, build_metrics};
    use crate::common::judge::BinaryJudgement;

    fn judgement(passed: bool, label: &str) -> BinaryJudgement {
        BinaryJudgement {
            passed,
            score: if passed { 1.0 } else { 0.0 },
            label: label.to_string(),
            metadata: serde_json::Value::Null,
        }
    }

    #[test]
    fn locomo_kioku_metrics_split_answer_and_retrieval_accuracy() {
        let metrics = build_metrics(
            &[
                LoCoMoKiokuMetricInput {
                    category: 1,
                    answer: BinaryJudgement {
                        passed: true,
                        score: 1.0,
                        label: "CORRECT".to_string(),
                        metadata: serde_json::json!({
                            "judge_model": "judge-model",
                        }),
                    },
                    retrieval: BinaryJudgement {
                        passed: false,
                        score: 0.0,
                        label: "INSUFFICIENT".to_string(),
                        metadata: serde_json::json!({
                            "judge_model": "judge-model",
                        }),
                    },
                    answerer_model: "answerer-model".to_string(),
                },
                LoCoMoKiokuMetricInput {
                    category: 1,
                    answer: judgement(false, "WRONG"),
                    retrieval: judgement(true, "SUFFICIENT"),
                    answerer_model: "answerer-model".to_string(),
                },
            ],
            "locomo.kioku.judge.answer.v1",
            "locomo.kioku.judge.retrieval.v1",
        );

        assert_eq!(metrics.protocol.as_deref(), Some("locomo_kioku_v1"));
        assert_eq!(metrics.metrics.overall_answer_accuracy, Some(0.5));
        assert_eq!(
            metrics.metrics.overall_retrieval_sufficiency_accuracy,
            Some(0.5)
        );
        assert_eq!(
            metrics.metrics.per_category_answer_accuracy["1"].accuracy,
            0.5
        );
        assert_eq!(
            metrics.metrics.per_category_retrieval_sufficiency_accuracy["1"].accuracy,
            0.5
        );
        assert_eq!(metrics.provenance.context_tokenizer, None);
    }
}
