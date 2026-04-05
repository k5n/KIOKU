use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievedMemory {
    pub event_id: String,
    pub stream_id: String,
    pub timestamp: String,
    pub content: String,
    pub speaker_id: Option<String>,
    pub speaker_name: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}
