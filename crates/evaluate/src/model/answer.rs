use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::prompt::PreparedPrompt;

#[derive(Debug, Clone)]
pub struct AnswerRequest<'a> {
    pub prompt: &'a PreparedPrompt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedAnswer {
    pub text: String,
    #[serde(default)]
    pub metadata: Value,
}
