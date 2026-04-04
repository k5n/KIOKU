use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct LoCoMoDataset {
    #[serde(rename = "fullContent")]
    pub contents: Vec<ConversationEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationEntry {
    pub qa: Vec<QA>,
    pub conversation: Conversation,
    pub event_summary: HashMap<String, SessionEvent>,
    pub observation: HashMap<String, HashMap<String, Vec<ObservationValue>>>,
    pub session_summary: HashMap<String, String>,
    pub sample_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QA {
    pub question: String,
    pub answer: Option<StringLike>,
    pub evidence: Vec<String>,
    pub category: u8,
    pub adversarial_answer: Option<StringLike>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringLike {
    Text(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

impl StringLike {
    pub fn as_string(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Integer(value) => value.to_string(),
            Self::Float(value) => value.to_string(),
            Self::Boolean(value) => value.to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Conversation {
    pub speaker_a: String,
    pub speaker_b: String,
    // セッションの日時情報などが動的なキー（session_1_date_time等）で入るため HashMap で対応
    #[serde(flatten)]
    pub sessions: HashMap<String, SessionContent>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionContent {
    // 文字列（session_X_date_time 用）
    DateTime(String),
    // 発言リスト（session_X 用）
    Messages(Vec<Message>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    pub speaker: String,
    pub text: String,
    pub dia_id: String,
    pub img_url: Option<Vec<String>>,
    pub blip_caption: Option<String>,
    pub query: Option<String>,
    #[serde(rename = "re-download")]
    pub re_download: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionEvent {
    #[serde(flatten)]
    pub speakers_events: HashMap<String, EventValue>,
    pub date: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventValue {
    // イベントの配列
    Events(Vec<String>),
    // 日付文字列（実際にはSessionEventのdateフィールドがこれに該当するが、flatten対策）
    Date(String),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ObservationValue {
    // [ "発言内容", "D1:3" ] のような形式
    TextAndId((String, String)),
    // [ "発言内容", ["D15:3", "D15:5"] ] のような形式
    TextAndIds((String, Vec<String>)),
}

fn main() -> anyhow::Result<()> {
    let json_data = std::fs::read_to_string("data/locomo10.json")
        .context("failed to read LoCoMo dataset file: locomo10.json")?;
    // JSONのルートが配列なので Vec<ConversationEntry> でデシリアライズします
    let dataset: Vec<ConversationEntry> =
        serde_json::from_str(&json_data).context("failed to parse LoCoMo dataset JSON")?;

    for entry in dataset {
        println!("ID: {}", entry.sample_id);
        println!("Speaker A: {}", entry.conversation.speaker_a);
    }

    Ok(())
}
