use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, RetrievedMemory};

#[derive(Debug, Clone, Copy)]
pub struct AnswerRequest<'a> {
    pub dataset: BenchmarkDataset,
    pub case: &'a BenchmarkCase,
    pub question: &'a BenchmarkQuestion,
    pub retrieved: &'a [RetrievedMemory],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneratedAnswer {
    pub text: String,
    #[serde(default)]
    pub metadata: Value,
}
