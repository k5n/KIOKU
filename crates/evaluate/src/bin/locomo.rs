use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// LoCoMo データセット全体を表すルート構造です。
/// `fullContent` 配列の各要素に 1 件分の会話サンプルが入ります。
#[derive(Debug, Serialize, Deserialize)]
pub struct LoCoMoDataset {
    #[serde(rename = "fullContent")]
    pub contents: Vec<ConversationEntry>,
}

/// LoCoMo の 1 サンプル分のデータです。
/// 会話本体、QA、イベント要約、観測情報、セッション要約をまとめて保持します。
#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationEntry {
    pub qa: Vec<QA>,
    pub conversation: Conversation,
    pub event_summary: HashMap<String, SessionEvent>,
    pub observation: HashMap<String, HashMap<String, Vec<ObservationValue>>>,
    pub session_summary: HashMap<String, String>,
    pub sample_id: String,
}

/// 1 つの質問応答データを表します。
/// 正解や根拠に加えて、カテゴリや adversarial answer も保持します。
#[derive(Debug, Serialize, Deserialize)]
pub struct QA {
    pub question: String,
    pub answer: Option<StringLike>,
    pub evidence: Vec<String>,
    pub category: u8,
    pub adversarial_answer: Option<StringLike>,
}

/// LoCoMo 内で文字列以外の JSON 値が混在する回答値を受けるための共用型です。
/// 文字列、整数、浮動小数、真偽値を 1 つの enum で扱います。
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

/// 会話参加者と、各セッションの内容をまとめた会話本体です。
/// セッション本文と日時が動的キーで表現されるため `flatten` で受けています。
#[derive(Debug, Serialize, Deserialize)]
pub struct Conversation {
    pub speaker_a: String,
    pub speaker_b: String,
    // セッションの日時情報などが動的なキー（session_1_date_time等）で入るため HashMap で対応
    #[serde(flatten)]
    pub sessions: HashMap<String, SessionContent>,
}

/// セッション関連の動的フィールドの値です。
/// 日時文字列か、発言メッセージ列のどちらかを取ります。
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionContent {
    // 文字列（session_X_date_time 用）
    DateTime(String),
    // 発言リスト（session_X 用）
    Messages(Vec<Message>),
}

/// 会話中の 1 発話を表します。
/// 発話者や本文に加えて、画像 URL や補助メタデータも保持します。
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

/// 1 セッション分のイベント要約です。
/// 話者ごとのイベント列と、そのセッションの日付を保持します。
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionEvent {
    #[serde(flatten)]
    pub speakers_events: HashMap<String, EventValue>,
    pub date: String,
}

/// `SessionEvent` 内の動的値です。
/// 話者ごとのイベント一覧、または日付文字列を受けるための補助型です。
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventValue {
    // イベントの配列
    Events(Vec<String>),
    // 日付文字列（実際にはSessionEventのdateフィールドがこれに該当するが、flatten対策）
    Date(String),
}

/// 観測情報の 1 要素です。
/// 観測テキストに対して、単一または複数の発話 ID を対応付けます。
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
