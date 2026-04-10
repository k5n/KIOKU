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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<u8>,
    pub retrieved_count: usize,
    #[serde(default)]
    pub retrieved_memory_ids: Vec<String>,
    #[serde(default)]
    pub retrieved_source_event_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_sufficient: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub judge_metadata: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_event_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_session_ids: Vec<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricProvenance {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_judge_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieval_judge_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judge_kind: Option<String>,
    pub metric_semantics_version: String,
    pub provisional: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locomo_overall_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_judge_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieval_judge_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_judge_prompt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retrieval_judge_prompt_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answerer_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_tokenizer: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub non_abstention_question_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstention_question_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scored_question_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overall_accuracy: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overall_answer_accuracy: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overall_retrieval_sufficiency_accuracy: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_averaged_answer_accuracy: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_averaged_retrieval_sufficiency_accuracy: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adversarial_accuracy: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstention_accuracy: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstention_answer_accuracy: Option<f32>,
    pub average_retrieved_item_count: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_context_token_count: Option<f32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub per_category_accuracy: BTreeMap<String, CategoryMetrics>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub per_category_answer_accuracy: BTreeMap<String, CategoryMetrics>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub per_category_retrieval_sufficiency_accuracy: BTreeMap<String, CategoryMetrics>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub per_type_accuracy: BTreeMap<String, CategoryMetrics>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub per_type_answer_accuracy: BTreeMap<String, CategoryMetrics>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub per_type_retrieval_sufficiency_accuracy: BTreeMap<String, CategoryMetrics>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsReport {
    pub dataset: BenchmarkDataset,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(flatten)]
    pub provenance: MetricProvenance,
    pub metrics: DatasetMetrics,
}
