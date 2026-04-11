use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PromptContextKind {
    MemoryPrompt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromptContext {
    pub kind: PromptContextKind,
    pub text: String,
    #[serde(default)]
    pub metadata: Value,
}
