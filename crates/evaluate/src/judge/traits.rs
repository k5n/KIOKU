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
