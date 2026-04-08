use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromptContextKind {
    RetrievedMemories,
    NoRetrieval,
    HistoryChats,
    HistoryChatsWithFacts,
    FactsOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptContext {
    pub kind: PromptContextKind,
    pub text: String,
    #[serde(default)]
    pub metadata: Value,
}
