use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{BenchmarkQuestion, GeneratedAnswer};
use crate::prompt::PromptContext;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BinaryJudgement {
    pub passed: bool,
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
    ) -> anyhow::Result<BinaryJudgement>;
}

#[async_trait]
pub trait AnswerJudge: Send + Sync {
    async fn judge_answer(
        &self,
        question: &BenchmarkQuestion,
        generated: &GeneratedAnswer,
    ) -> anyhow::Result<BinaryJudgement>;
}

#[async_trait]
pub trait RetrievalJudge: Send + Sync {
    async fn judge_retrieval(
        &self,
        question: &BenchmarkQuestion,
        context: &PromptContext,
    ) -> anyhow::Result<BinaryJudgement>;
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
