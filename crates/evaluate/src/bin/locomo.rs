use anyhow::{Context, anyhow, bail};
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

/// 動的キーから復元した 1 セッション分の参照です。
/// `session_N` と `session_N_date_time` の 1:1 対応が取れたものだけを返します。
#[derive(Debug, Clone)]
pub struct LoCoMoSessionRef<'a> {
    pub session_number: usize,
    pub session_id: String,
    pub start_time: &'a str,
    pub messages: &'a [Message],
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

impl Conversation {
    /// 動的キーをパースしてセッション一覧を復元します。
    /// `session_N` と `session_N_date_time` の両方が存在することを検証し、
    /// セッション番号で昇順ソートして返します。
    pub fn ordered_sessions(&self) -> anyhow::Result<Vec<LoCoMoSessionRef<'_>>> {
        let mut starts = HashMap::new();
        let mut messages = HashMap::new();

        for (key, value) in &self.sessions {
            let parsed = ParsedSessionKey::parse(key)?;
            match (parsed.kind, value) {
                (ParsedSessionKeyKind::StartTime, SessionContent::DateTime(start_time)) => {
                    starts.insert(parsed.session_number, start_time.as_str());
                }
                (ParsedSessionKeyKind::Messages, SessionContent::Messages(session_messages)) => {
                    messages.insert(parsed.session_number, session_messages.as_slice());
                }
                (ParsedSessionKeyKind::StartTime, SessionContent::Messages(_)) => {
                    bail!("expected datetime string for key `{key}`")
                }
                (ParsedSessionKeyKind::Messages, SessionContent::DateTime(_)) => {
                    bail!("expected message array for key `{key}`")
                }
            }
        }

        let mut session_numbers: Vec<_> = starts.keys().chain(messages.keys()).copied().collect();
        session_numbers.sort_unstable();
        session_numbers.dedup();

        let mut ordered_sessions = Vec::with_capacity(session_numbers.len());
        for session_number in session_numbers {
            let session_id = format!("session_{session_number}");
            let start_time = starts
                .get(&session_number)
                .copied()
                .ok_or_else(|| anyhow!("missing `{session_id}_date_time` for `{session_id}`"))?;
            let session_messages = messages
                .get(&session_number)
                .copied()
                .ok_or_else(|| anyhow!("missing `{session_id}` messages"))?;

            ordered_sessions.push(LoCoMoSessionRef {
                session_number,
                session_id,
                start_time,
                messages: session_messages,
            });
        }

        Ok(ordered_sessions)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedSessionKeyKind {
    Messages,
    StartTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedSessionKey {
    session_number: usize,
    kind: ParsedSessionKeyKind,
}

impl ParsedSessionKey {
    fn parse(key: &str) -> anyhow::Result<Self> {
        let suffix = "_date_time";
        let (base, kind) = if let Some(base) = key.strip_suffix(suffix) {
            (base, ParsedSessionKeyKind::StartTime)
        } else {
            (key, ParsedSessionKeyKind::Messages)
        };

        let number = base
            .strip_prefix("session_")
            .ok_or_else(|| anyhow!("unexpected conversation session key: `{key}`"))?
            .parse::<usize>()
            .with_context(|| format!("failed to parse session number from key `{key}`"))?;

        Ok(Self {
            session_number: number,
            kind,
        })
    }
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
        if let Some(first_session) = entry.conversation.ordered_sessions()?.first() {
            println!("First Session ID: {}", first_session.session_id);
            println!("First Session Date: {}", first_session.start_time);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(dia_id: &str) -> Message {
        Message {
            speaker: "A".to_string(),
            text: "hello".to_string(),
            dia_id: dia_id.to_string(),
            img_url: None,
            blip_caption: None,
            query: None,
            re_download: None,
        }
    }

    #[test]
    fn orders_sessions_by_numeric_session_id() {
        let conversation = Conversation {
            speaker_a: "A".to_string(),
            speaker_b: "B".to_string(),
            sessions: HashMap::from([
                (
                    "session_10".to_string(),
                    SessionContent::Messages(vec![message("D10")]),
                ),
                (
                    "session_10_date_time".to_string(),
                    SessionContent::DateTime("2024-01-10 09:00".to_string()),
                ),
                (
                    "session_2".to_string(),
                    SessionContent::Messages(vec![message("D2")]),
                ),
                (
                    "session_2_date_time".to_string(),
                    SessionContent::DateTime("2024-01-02 09:00".to_string()),
                ),
            ]),
        };

        let ordered = conversation.ordered_sessions().unwrap();

        assert_eq!(ordered.len(), 2);
        assert_eq!(ordered[0].session_number, 2);
        assert_eq!(ordered[0].session_id, "session_2");
        assert_eq!(ordered[1].session_number, 10);
        assert_eq!(ordered[1].session_id, "session_10");
    }

    #[test]
    fn fails_when_session_start_time_is_missing() {
        let conversation = Conversation {
            speaker_a: "A".to_string(),
            speaker_b: "B".to_string(),
            sessions: HashMap::from([(
                "session_1".to_string(),
                SessionContent::Messages(vec![message("D1")]),
            )]),
        };

        let error = conversation.ordered_sessions().unwrap_err().to_string();
        assert!(error.contains("missing `session_1_date_time`"));
    }
}
