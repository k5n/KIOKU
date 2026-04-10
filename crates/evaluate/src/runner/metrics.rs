use crate::judge::BinaryJudgement;
use crate::model::{
    BenchmarkDataset, CategoryMetrics, DatasetMetrics, MetricProvenance, MetricsReport,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub(super) struct LoCoMoKiokuMetricInput {
    pub category: u8,
    pub answer: BinaryJudgement,
    pub retrieval: BinaryJudgement,
    pub retrieved_count: usize,
    pub answerer_model: String,
}

#[derive(Debug, Clone)]
pub(super) struct LongMemEvalKiokuMetricInput {
    pub question_type: String,
    pub is_abstention: bool,
    pub answer: BinaryJudgement,
    pub retrieval: BinaryJudgement,
    pub retrieved_count: usize,
    pub context_token_count: usize,
    pub answerer_model: String,
}

pub(super) fn build_locomo_kioku_metrics(
    inputs: &[LoCoMoKiokuMetricInput],
    answer_judge_prompt_id: &str,
    retrieval_judge_prompt_id: &str,
) -> MetricsReport {
    let mut per_category_answer_accuracy = BTreeMap::new();
    let mut per_category_retrieval_sufficiency_accuracy = BTreeMap::new();

    let mut answer_correct = 0usize;
    let mut retrieval_correct = 0usize;
    let mut retrieved_total = 0usize;

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

        retrieved_total += input.retrieved_count;
    }

    finalize_category_metrics(&mut per_category_answer_accuracy);
    finalize_category_metrics(&mut per_category_retrieval_sufficiency_accuracy);

    let average_retrieved_item_count = if inputs.is_empty() {
        0.0
    } else {
        retrieved_total as f32 / inputs.len() as f32
    };
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
            average_retrieved_item_count,
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

pub(super) fn build_longmemeval_kioku_metrics(
    inputs: &[LongMemEvalKiokuMetricInput],
    answer_judge_prompt_id: &str,
    retrieval_judge_prompt_id: &str,
    context_tokenizer: &str,
) -> MetricsReport {
    let mut per_type_answer_accuracy = BTreeMap::new();
    let mut per_type_retrieval_sufficiency_accuracy = BTreeMap::new();

    let mut question_count = 0usize;
    let mut non_abstention_question_count = 0usize;
    let mut abstention_question_count = 0usize;
    let mut non_abstention_answer_correct = 0usize;
    let mut non_abstention_retrieval_correct = 0usize;
    let mut abstention_answer_correct = 0usize;
    let mut retrieved_total = 0usize;
    let mut context_token_total = 0usize;

    for input in inputs {
        question_count += 1;
        retrieved_total += input.retrieved_count;
        context_token_total += input.context_token_count;

        if input.is_abstention {
            abstention_question_count += 1;
            if input.answer.passed {
                abstention_answer_correct += 1;
            }
            continue;
        }

        non_abstention_question_count += 1;
        if input.answer.passed {
            non_abstention_answer_correct += 1;
        }
        if input.retrieval.passed {
            non_abstention_retrieval_correct += 1;
        }

        let answer_entry = per_type_answer_accuracy
            .entry(input.question_type.clone())
            .or_insert_with(empty_category_metrics);
        answer_entry.total += 1;
        if input.answer.passed {
            answer_entry.correct += 1;
        }

        let retrieval_entry = per_type_retrieval_sufficiency_accuracy
            .entry(input.question_type.clone())
            .or_insert_with(empty_category_metrics);
        retrieval_entry.total += 1;
        if input.retrieval.passed {
            retrieval_entry.correct += 1;
        }
    }

    finalize_category_metrics(&mut per_type_answer_accuracy);
    finalize_category_metrics(&mut per_type_retrieval_sufficiency_accuracy);

    let answer_metadata = inputs.first().map(|input| &input.answer.metadata);
    let retrieval_metadata = inputs.first().map(|input| &input.retrieval.metadata);

    MetricsReport {
        dataset: BenchmarkDataset::LongMemEval,
        protocol: Some("longmemeval_kioku_v1".to_string()),
        provenance: MetricProvenance {
            answer_judge_kind: Some("longmemeval_kioku_answer_llm".to_string()),
            retrieval_judge_kind: Some("longmemeval_kioku_retrieval_llm".to_string()),
            judge_kind: None,
            metric_semantics_version: "longmemeval_kioku_v1".to_string(),
            provisional: false,
            locomo_overall_scope: None,
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
            context_tokenizer: Some(context_tokenizer.to_string()),
        },
        metrics: DatasetMetrics {
            question_count,
            non_abstention_question_count: Some(non_abstention_question_count),
            abstention_question_count: Some(abstention_question_count),
            scored_question_count: None,
            overall_accuracy: None,
            overall_answer_accuracy: (non_abstention_question_count > 0)
                .then(|| ratio(non_abstention_answer_correct, non_abstention_question_count)),
            overall_retrieval_sufficiency_accuracy: (non_abstention_question_count > 0).then(
                || {
                    ratio(
                        non_abstention_retrieval_correct,
                        non_abstention_question_count,
                    )
                },
            ),
            task_averaged_answer_accuracy: mean_accuracy(&per_type_answer_accuracy),
            task_averaged_retrieval_sufficiency_accuracy: mean_accuracy(
                &per_type_retrieval_sufficiency_accuracy,
            ),
            adversarial_accuracy: None,
            abstention_accuracy: None,
            abstention_answer_accuracy: (abstention_question_count > 0)
                .then(|| ratio(abstention_answer_correct, abstention_question_count)),
            average_retrieved_item_count: if question_count == 0 {
                0.0
            } else {
                retrieved_total as f32 / question_count as f32
            },
            average_context_token_count: (question_count > 0)
                .then(|| context_token_total as f32 / question_count as f32),
            per_category_accuracy: BTreeMap::new(),
            per_category_answer_accuracy: BTreeMap::new(),
            per_category_retrieval_sufficiency_accuracy: BTreeMap::new(),
            per_type_accuracy: BTreeMap::new(),
            per_type_answer_accuracy,
            per_type_retrieval_sufficiency_accuracy,
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

fn mean_accuracy(metrics: &BTreeMap<String, CategoryMetrics>) -> Option<f32> {
    (!metrics.is_empty())
        .then(|| metrics.values().map(|metric| metric.accuracy).sum::<f32>() / metrics.len() as f32)
}

#[cfg(test)]
mod tests {
    use super::{
        LoCoMoKiokuMetricInput, LongMemEvalKiokuMetricInput, build_locomo_kioku_metrics,
        build_longmemeval_kioku_metrics,
    };
    use crate::judge::BinaryJudgement;

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
        let metrics = build_locomo_kioku_metrics(
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
                    retrieved_count: 4,
                    answerer_model: "answerer-model".to_string(),
                },
                LoCoMoKiokuMetricInput {
                    category: 1,
                    answer: judgement(false, "WRONG"),
                    retrieval: judgement(true, "SUFFICIENT"),
                    retrieved_count: 2,
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
        assert_eq!(metrics.metrics.average_retrieved_item_count, 3.0);
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

    #[test]
    fn longmemeval_kioku_metrics_split_main_and_abstention_scores() {
        let metrics = build_longmemeval_kioku_metrics(
            &[
                LongMemEvalKiokuMetricInput {
                    question_type: "multi-session".to_string(),
                    is_abstention: false,
                    answer: judgement(true, "CORRECT"),
                    retrieval: judgement(false, "INSUFFICIENT"),
                    retrieved_count: 3,
                    context_token_count: 30,
                    answerer_model: "answerer-model".to_string(),
                },
                LongMemEvalKiokuMetricInput {
                    question_type: "knowledge-update".to_string(),
                    is_abstention: false,
                    answer: judgement(false, "WRONG"),
                    retrieval: judgement(true, "SUFFICIENT"),
                    retrieved_count: 1,
                    context_token_count: 10,
                    answerer_model: "answerer-model".to_string(),
                },
                LongMemEvalKiokuMetricInput {
                    question_type: "multi-session".to_string(),
                    is_abstention: true,
                    answer: judgement(true, "CORRECT"),
                    retrieval: judgement(true, "SUFFICIENT"),
                    retrieved_count: 2,
                    context_token_count: 20,
                    answerer_model: "answerer-model".to_string(),
                },
            ],
            "longmemeval.kioku.judge.answer.v1",
            "longmemeval.kioku.judge.retrieval.v1",
            "whitespace_v1",
        );

        assert_eq!(metrics.protocol.as_deref(), Some("longmemeval_kioku_v1"));
        assert_eq!(metrics.metrics.question_count, 3);
        assert_eq!(metrics.metrics.non_abstention_question_count, Some(2));
        assert_eq!(metrics.metrics.abstention_question_count, Some(1));
        assert_eq!(metrics.metrics.overall_answer_accuracy, Some(0.5));
        assert_eq!(
            metrics.metrics.overall_retrieval_sufficiency_accuracy,
            Some(0.5)
        );
        assert_eq!(metrics.metrics.abstention_answer_accuracy, Some(1.0));
        assert_eq!(metrics.metrics.task_averaged_answer_accuracy, Some(0.5));
        assert_eq!(
            metrics.metrics.task_averaged_retrieval_sufficiency_accuracy,
            Some(0.5)
        );
        assert_eq!(metrics.metrics.average_context_token_count, Some(20.0));
        assert_eq!(
            metrics.metrics.per_type_answer_accuracy["knowledge-update"].accuracy,
            0.0
        );
        assert_eq!(
            metrics.metrics.per_type_retrieval_sufficiency_accuracy["multi-session"].accuracy,
            0.0
        );
        assert_eq!(
            metrics.provenance.context_tokenizer.as_deref(),
            Some("whitespace_v1")
        );
    }
}
