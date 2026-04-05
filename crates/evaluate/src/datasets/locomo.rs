use anyhow::{Context, anyhow, bail};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

use crate::model::{
    BenchmarkCase, BenchmarkDataset, BenchmarkEvent, BenchmarkQuestion, GoldAnswerVariant,
};

pub type LoCoMoDataset = Vec<ConversationEntry>;

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
    #[serde(flatten)]
    pub sessions: HashMap<String, SessionContent>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionContent {
    DateTime(String),
    Messages(Vec<Message>),
}

#[derive(Debug, Clone)]
pub struct LoCoMoSessionRef<'a> {
    pub session_number: usize,
    pub session_id: String,
    pub start_time: &'a str,
    pub messages: &'a [Message],
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
    Events(Vec<String>),
    Date(String),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ObservationValue {
    TextAndId((String, String)),
    TextAndIds((String, Vec<String>)),
}

impl Conversation {
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

        for session_number in messages.keys() {
            if !starts.contains_key(session_number) {
                let session_id = format!("session_{session_number}");
                anyhow::bail!("missing `{session_id}_date_time` for `{session_id}`");
            }
        }

        let mut session_numbers: Vec<_> = starts
            .keys()
            .filter(|session_number| messages.contains_key(session_number))
            .copied()
            .collect();
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

pub fn load_locomo_dataset(path: &str) -> anyhow::Result<LoCoMoDataset> {
    let json_data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read LoCoMo dataset file: {path}"))?;
    serde_json::from_str(&json_data).context("failed to parse LoCoMo dataset JSON")
}

pub fn adapt_locomo_entry(entry: &ConversationEntry) -> anyhow::Result<BenchmarkCase> {
    let case_id = format!("locomo:{}", entry.sample_id);
    let ordered_sessions = entry.conversation.ordered_sessions()?;
    let session_times: Vec<_> = ordered_sessions
        .iter()
        .map(|session| parse_locomo_datetime(session.start_time))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let mut events = Vec::new();
    for (index, session) in ordered_sessions.iter().enumerate() {
        let start = session_times[index];
        let interval = if let Some(next_start) = session_times.get(index + 1).copied() {
            session_interval(start, next_start, session.messages.len())
        } else {
            Duration::seconds(30)
        };

        for (turn_idx, message) in session.messages.iter().enumerate() {
            let timestamp = start + interval * (turn_idx as i32);
            events.push(BenchmarkEvent {
                event_id: format!("locomo:{}:event:{}", entry.sample_id, message.dia_id),
                stream_id: session.session_id.clone(),
                timestamp: timestamp.to_rfc3339(),
                content: message.text.clone(),
                speaker_id: Some(message.speaker.clone()),
                speaker_name: Some(message.speaker.clone()),
                metadata: json!({
                    "dataset": "locomo",
                    "session_id": session.session_id,
                    "session_number": session.session_number,
                    "dia_id": message.dia_id,
                }),
            });
        }
    }

    let last_timestamp = events.last().map(|event| event.timestamp.clone());
    let questions = entry
        .qa
        .iter()
        .enumerate()
        .map(|(idx, qa)| {
            let gold_answer_variant = if qa.category == 5 && qa.adversarial_answer.is_some() {
                GoldAnswerVariant::Adversarial
            } else {
                GoldAnswerVariant::Default
            };

            let gold_answer = match gold_answer_variant {
                GoldAnswerVariant::Adversarial => qa
                    .adversarial_answer
                    .as_ref()
                    .map(StringLike::as_string)
                    .or_else(|| qa.answer.as_ref().map(StringLike::as_string)),
                GoldAnswerVariant::Default => qa.answer.as_ref().map(StringLike::as_string),
            }
            .into_iter()
            .collect();

            BenchmarkQuestion {
                question_id: format!("locomo:{}:q{idx}", entry.sample_id),
                question: qa.question.clone(),
                question_timestamp: last_timestamp.clone(),
                gold_answers: gold_answer,
                evidence_event_ids: qa
                    .evidence
                    .iter()
                    .map(|dia_id| format!("locomo:{}:event:{dia_id}", entry.sample_id))
                    .collect(),
                evidence_session_ids: Vec::new(),
                category: Some(qa.category),
                question_type: None,
                gold_answer_variant,
                is_abstention: false,
                metadata: json!({
                    "dataset": "locomo",
                    "sample_id": entry.sample_id,
                    "category_name": locomo_category_name(qa.category),
                }),
            }
        })
        .collect();

    Ok(BenchmarkCase {
        dataset: BenchmarkDataset::LoCoMo,
        case_id,
        events,
        questions,
        metadata: json!({
            "dataset": "locomo",
            "sample_id": entry.sample_id,
            "speaker_a": entry.conversation.speaker_a,
            "speaker_b": entry.conversation.speaker_b,
        }),
    })
}

fn parse_locomo_datetime(value: &str) -> anyhow::Result<DateTime<Utc>> {
    const FORMATS: &[&str] = &[
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d %H:%M",
        "%I:%M %P on %-d %B, %Y",
        "%I:%M %P on %d %B, %Y",
        "%I:%M %p on %-d %B, %Y",
        "%I:%M %p on %d %B, %Y",
    ];

    for format in FORMATS {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(value, format) {
            return Ok(DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc));
        }
    }

    anyhow::bail!("failed to parse LoCoMo datetime `{value}`")
}

fn session_interval(
    start: DateTime<Utc>,
    next_start: DateTime<Utc>,
    message_count: usize,
) -> Duration {
    if message_count <= 1 {
        return Duration::seconds(30);
    }

    let desired = Duration::seconds(30);
    let span = next_start - start;
    let max_interval = span / (message_count as i32);
    if max_interval > Duration::zero() && max_interval < desired {
        max_interval
    } else {
        desired
    }
}

fn locomo_category_name(category: u8) -> &'static str {
    match category {
        1 => "multi_hop",
        2 => "temporal",
        3 => "open_domain",
        4 => "single_hop",
        5 => "adversarial",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(dia_id: &str, speaker: &str, text: &str) -> Message {
        Message {
            speaker: speaker.to_string(),
            text: text.to_string(),
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
                    SessionContent::Messages(vec![message("D10", "A", "ten")]),
                ),
                (
                    "session_10_date_time".to_string(),
                    SessionContent::DateTime("2024-01-10 09:00".to_string()),
                ),
                (
                    "session_2".to_string(),
                    SessionContent::Messages(vec![message("D2", "A", "two")]),
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
        assert_eq!(ordered[1].session_number, 10);
    }

    #[test]
    fn fails_when_session_start_time_is_missing() {
        let conversation = Conversation {
            speaker_a: "A".to_string(),
            speaker_b: "B".to_string(),
            sessions: HashMap::from([(
                "session_1".to_string(),
                SessionContent::Messages(vec![message("D1", "A", "hello")]),
            )]),
        };

        let error = conversation.ordered_sessions().unwrap_err().to_string();
        assert!(error.contains("missing `session_1_date_time`"));
    }

    #[test]
    fn ignores_orphan_datetime_sessions_from_official_dataset() {
        let conversation = Conversation {
            speaker_a: "A".to_string(),
            speaker_b: "B".to_string(),
            sessions: HashMap::from([
                (
                    "session_1".to_string(),
                    SessionContent::Messages(vec![message("D1", "A", "hello")]),
                ),
                (
                    "session_1_date_time".to_string(),
                    SessionContent::DateTime("2024-01-01 09:00".to_string()),
                ),
                (
                    "session_2_date_time".to_string(),
                    SessionContent::DateTime("2024-01-02 09:00".to_string()),
                ),
            ]),
        };

        let ordered = conversation.ordered_sessions().unwrap();

        assert_eq!(ordered.len(), 1);
        assert_eq!(ordered[0].session_id, "session_1");
    }

    #[test]
    fn adapts_entry_with_monotonic_timestamps_and_canonical_ids() {
        let entry = ConversationEntry {
            qa: vec![QA {
                question: "Where did they meet?".to_string(),
                answer: Some(StringLike::Text("At the cafe".to_string())),
                evidence: vec!["D1:1".to_string(), "D1:2".to_string()],
                category: 1,
                adversarial_answer: None,
            }],
            conversation: Conversation {
                speaker_a: "Alice".to_string(),
                speaker_b: "Bob".to_string(),
                sessions: HashMap::from([
                    (
                        "session_2".to_string(),
                        SessionContent::Messages(vec![message("D2:1", "Alice", "Later")]),
                    ),
                    (
                        "session_2_date_time".to_string(),
                        SessionContent::DateTime("2024-01-02 09:00".to_string()),
                    ),
                    (
                        "session_1".to_string(),
                        SessionContent::Messages(vec![
                            message("D1:1", "Alice", "Hi"),
                            message("D1:2", "Bob", "Hello"),
                        ]),
                    ),
                    (
                        "session_1_date_time".to_string(),
                        SessionContent::DateTime("2024-01-01 09:00".to_string()),
                    ),
                ]),
            },
            event_summary: HashMap::new(),
            observation: HashMap::new(),
            session_summary: HashMap::new(),
            sample_id: "sample-1".to_string(),
        };

        let case = adapt_locomo_entry(&entry).unwrap();

        assert_eq!(case.case_id, "locomo:sample-1");
        assert_eq!(case.questions[0].question_id, "locomo:sample-1:q0");
        assert_eq!(
            case.questions[0].evidence_event_ids,
            vec![
                "locomo:sample-1:event:D1:1".to_string(),
                "locomo:sample-1:event:D1:2".to_string()
            ]
        );
        assert_eq!(case.events[0].event_id, "locomo:sample-1:event:D1:1");
        assert!(case.events[0].timestamp < case.events[1].timestamp);
        assert!(case.events[1].timestamp < case.events[2].timestamp);
    }
}
