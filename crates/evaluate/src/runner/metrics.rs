use crate::judge::Judgement;
use crate::model::{
    BenchmarkDataset, CategoryMetrics, DatasetMetrics, MetricProvenance, MetricsReport,
};
use std::collections::BTreeMap;

pub(super) fn build_metrics(
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

#[cfg(test)]
mod tests {
    use super::build_metrics;
    use crate::judge::Judgement;
    use crate::model::{BenchmarkDataset, BenchmarkQuestion, GoldAnswerVariant};

    fn judgement(is_correct: bool, label: &str) -> Judgement {
        Judgement {
            is_correct,
            score: if is_correct { 1.0 } else { 0.0 },
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
        assert_eq!(metrics.metrics.scored_question_count, 1);
        assert_eq!(metrics.metrics.overall_accuracy, 1.0);
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

        assert_eq!(metrics.metrics.overall_accuracy, 2.0 / 3.0);
        assert_eq!(metrics.metrics.abstention_accuracy, Some(1.0));
        assert_eq!(metrics.metrics.per_category_accuracy["1"].accuracy, 0.5);
        assert_eq!(metrics.metrics.per_category_accuracy["2"].accuracy, 1.0);
        assert_eq!(metrics.metrics.per_type_accuracy["type-a"].accuracy, 0.5);
        assert_eq!(metrics.metrics.per_type_accuracy["type-b"].accuracy, 1.0);
    }
}
