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
    pub prompt_context: PromptContext,
    #[serde(default)]
    pub metadata: Value,
}
