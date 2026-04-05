use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

use super::benchmark::BenchmarkDataset;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnswerLogRecord {
    pub dataset: BenchmarkDataset,
    pub case_id: String,
    pub question_id: String,
    pub question: String,
    pub generated_answer: String,
    pub gold_answers: Vec<String>,
    pub is_correct: bool,
    pub score: f32,
    pub label: String,
    pub question_type: Option<String>,
    pub category: Option<u8>,
    pub is_abstention: bool,
    #[serde(default)]
    pub answer_metadata: Value,
    #[serde(default)]
    pub judgement_metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalLogRecord {
    pub dataset: BenchmarkDataset,
    pub case_id: String,
    pub question_id: String,
    pub retrieved_count: usize,
    pub retrieved_event_ids: Vec<String>,
    pub evidence_event_ids: Vec<String>,
    pub evidence_session_ids: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricProvenance {
    pub judge_kind: String,
    pub metric_semantics_version: String,
    pub provisional: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locomo_overall_scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CategoryMetrics {
    pub correct: usize,
    pub total: usize,
    pub accuracy: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatasetMetrics {
    pub question_count: usize,
    pub scored_question_count: usize,
    pub overall_accuracy: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adversarial_accuracy: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstention_accuracy: Option<f32>,
    pub average_retrieved_item_count: f32,
    #[serde(default)]
    pub per_category_accuracy: BTreeMap<String, CategoryMetrics>,
    #[serde(default)]
    pub per_type_accuracy: BTreeMap<String, CategoryMetrics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsReport {
    pub dataset: BenchmarkDataset,
    #[serde(flatten)]
    pub provenance: MetricProvenance,
    pub metrics: DatasetMetrics,
}
