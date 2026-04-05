use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BenchmarkDataset {
    #[serde(rename = "locomo")]
    LoCoMo,
    #[serde(rename = "longmemeval")]
    LongMemEval,
}

impl BenchmarkDataset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LoCoMo => "locomo",
            Self::LongMemEval => "longmemeval",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvalScope {
    pub dataset: BenchmarkDataset,
    pub case_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkCase {
    pub dataset: BenchmarkDataset,
    pub case_id: String,
    pub events: Vec<BenchmarkEvent>,
    pub questions: Vec<BenchmarkQuestion>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkEvent {
    pub event_id: String,
    pub stream_id: String,
    pub timestamp: String,
    pub content: String,
    pub speaker_id: Option<String>,
    pub speaker_name: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkQuestion {
    pub question_id: String,
    pub question: String,
    pub question_timestamp: Option<String>,
    pub gold_answers: Vec<String>,
    #[serde(default)]
    pub evidence_event_ids: Vec<String>,
    #[serde(default)]
    pub evidence_session_ids: Vec<String>,
    pub category: Option<u8>,
    pub question_type: Option<String>,
    pub gold_answer_variant: GoldAnswerVariant,
    pub is_abstention: bool,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoldAnswerVariant {
    Default,
    Adversarial,
}

#[cfg(test)]
mod tests {
    use super::BenchmarkDataset;

    #[test]
    fn benchmark_dataset_serializes_to_canonical_names() {
        assert_eq!(
            serde_json::to_string(&BenchmarkDataset::LoCoMo).unwrap(),
            "\"locomo\""
        );
        assert_eq!(
            serde_json::to_string(&BenchmarkDataset::LongMemEval).unwrap(),
            "\"longmemeval\""
        );
    }
}
