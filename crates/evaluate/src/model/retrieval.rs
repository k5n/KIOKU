use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::prompt::PromptContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RetrievalBudget {
    pub max_items: Option<usize>,
    pub max_tokens: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryInput {
    pub scope: super::benchmark::EvalScope,
    pub question_id: String,
    pub query: String,
    pub timestamp: Option<String>,
    pub budget: RetrievalBudget,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryOutput {
    pub retrieved: Vec<RetrievedMemory>,
    #[serde(default)]
    pub prompt_context: Option<PromptContext>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievedMemory {
    pub memory_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    pub content: String,
    #[serde(default)]
    pub metadata: Value,
}
