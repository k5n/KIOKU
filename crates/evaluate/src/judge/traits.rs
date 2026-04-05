use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{BenchmarkQuestion, GeneratedAnswer};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Judgement {
    pub is_correct: bool,
    pub score: f32,
    pub label: String,
    #[serde(default)]
    pub metadata: Value,
}

#[async_trait]
pub trait Judge {
    async fn judge(
        &self,
        question: &BenchmarkQuestion,
        generated: &GeneratedAnswer,
    ) -> anyhow::Result<Judgement>;
}

pub(crate) fn normalize_text(input: &str) -> String {
    input
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

pub(crate) fn provisional_metadata(judge_kind: &str) -> Value {
    serde_json::json!({
        "judge_kind": judge_kind,
        "metric_semantics_version": "phase1-minimal-v1",
        "provisional": true,
    })
}
