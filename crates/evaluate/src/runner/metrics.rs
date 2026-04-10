use crate::judge::BinaryJudgement;
use crate::model::{
    BenchmarkDataset, CategoryMetrics, DatasetMetrics, MetricProvenance, MetricsReport,
};
use std::collections::BTreeMap;

pub(super) fn build_metrics(
    dataset: BenchmarkDataset,
    judgements: &[(&crate::model::BenchmarkQuestion, BinaryJudgement)],
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
        .filter(|(_, judgement)| judgement.passed)
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
                .or_insert_with(empty_category_metrics);
            entry.total += 1;
            if judgement.passed {
                entry.correct += 1;
            }

            if category == 5 {
                adversarial_total += 1;
                if judgement.passed {
                    adversarial_correct += 1;
                }
            }
        }

        if let Some(question_type) = &question.question_type {
            let entry = per_type_accuracy
                .entry(question_type.clone())
                .or_insert_with(empty_category_metrics);
            entry.total += 1;
            if judgement.passed {
                entry.correct += 1;
            }
        }

        if question.is_abstention {
            abstention_total += 1;
            if judgement.passed {
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
        protocol: None,
        provenance: MetricProvenance {
            answer_judge_kind: None,
            retrieval_judge_kind: None,
            judge_kind: Some(match dataset {
                BenchmarkDataset::LoCoMo => "locomo_exact_match".to_string(),
                BenchmarkDataset::LongMemEval => "longmemeval_exact_match".to_string(),
            }),
            metric_semantics_version: "phase1-minimal-v1".to_string(),
            provisional: true,
            locomo_overall_scope: matches!(dataset, BenchmarkDataset::LoCoMo)
                .then(|| "category_1_4".to_string()),
            answer_judge_model: None,
            retrieval_judge_model: None,
            answer_judge_prompt_id: None,
            retrieval_judge_prompt_id: None,
            answerer_model: None,
            context_tokenizer: None,
        },
        metrics: DatasetMetrics {
            question_count: judgements.len(),
            non_abstention_question_count: None,
            abstention_question_count: None,
            scored_question_count: Some(overall_total),
            overall_accuracy: Some(ratio(overall_correct, overall_total)),
            overall_answer_accuracy: None,
            overall_retrieval_sufficiency_accuracy: None,
            task_averaged_answer_accuracy: None,
            task_averaged_retrieval_sufficiency_accuracy: None,
            adversarial_accuracy: (adversarial_total > 0)
                .then(|| ratio(adversarial_correct, adversarial_total)),
            abstention_accuracy: (abstention_total > 0)
                .then(|| ratio(abstention_correct, abstention_total)),
            abstention_answer_accuracy: None,
            average_retrieved_item_count,
            average_context_token_count: None,
            per_category_accuracy,
            per_category_answer_accuracy: BTreeMap::new(),
            per_category_retrieval_sufficiency_accuracy: BTreeMap::new(),
            per_type_accuracy,
            per_type_answer_accuracy: BTreeMap::new(),
            per_type_retrieval_sufficiency_accuracy: BTreeMap::new(),
        },
    }
}

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
        build_longmemeval_kioku_metrics, build_metrics,
    };
    use crate::judge::BinaryJudgement;
    use crate::model::{BenchmarkDataset, BenchmarkQuestion, GoldAnswerVariant};

    fn judgement(passed: bool, label: &str) -> BinaryJudgement {
        BinaryJudgement {
            passed,
            score: if passed { 1.0 } else { 0.0 },
            label: label.to_string(),
            metadata: serde_json::Value::Null,
        }
    }

    fn question(
        category: Option<u8>,
        question_type: Option<&str>,
        is_abstention: bool,
    ) -> BenchmarkQuestion {
        BenchmarkQuestion {
            question_id: "q".to_string(),
            question: "Q".to_string(),
            question_timestamp: None,
            gold_answers: vec!["answer".to_string()],
            evidence_event_ids: Vec::new(),
            evidence_session_ids: Vec::new(),
            category,
            question_type: question_type.map(ToString::to_string),
            gold_answer_variant: GoldAnswerVariant::Default,
            is_abstention,
            metadata: serde_json::Value::Null,
        }
    }

    #[test]
    fn locomo_metrics_exclude_category_five_from_overall_accuracy() {
        let q1 = question(Some(1), None, false);
        let q2 = question(Some(5), None, false);
        let judgements = vec![
            (&q1, judgement(true, "correct")),
            (&q2, judgement(false, "incorrect")),
        ];

        let metrics = build_metrics(BenchmarkDataset::LoCoMo, &judgements, &[1.0, 2.0]);

        assert_eq!(metrics.metrics.question_count, 2);
        assert_eq!(metrics.metrics.scored_question_count, Some(1));
        assert_eq!(metrics.metrics.overall_accuracy, Some(1.0));
        assert_eq!(metrics.metrics.adversarial_accuracy, Some(0.0));
        assert_eq!(metrics.metrics.average_retrieved_item_count, 1.5);
    }

    #[test]
    fn metrics_track_category_type_and_abstention_accuracy() {
        let q1 = question(Some(1), Some("type-a"), true);
        let q2 = question(Some(1), Some("type-a"), false);
        let q3 = question(Some(2), Some("type-b"), false);
        let judgements = vec![
            (&q1, judgement(true, "correct")),
            (&q2, judgement(false, "incorrect")),
            (&q3, judgement(true, "correct")),
        ];

        let metrics = build_metrics(BenchmarkDataset::LongMemEval, &judgements, &[]);

        assert_eq!(metrics.metrics.overall_accuracy, Some(2.0 / 3.0));
        assert_eq!(metrics.metrics.abstention_accuracy, Some(1.0));
        assert_eq!(metrics.metrics.per_category_accuracy["1"].accuracy, 0.5);
        assert_eq!(metrics.metrics.per_category_accuracy["2"].accuracy, 1.0);
        assert_eq!(metrics.metrics.per_type_accuracy["type-a"].accuracy, 0.5);
        assert_eq!(metrics.metrics.per_type_accuracy["type-b"].accuracy, 1.0);
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
