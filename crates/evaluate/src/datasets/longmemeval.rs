use anyhow::{Context, anyhow, ensure};
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

use crate::model::{
    BenchmarkCase, BenchmarkDataset, BenchmarkEvent, BenchmarkQuestion, GoldAnswerVariant,
};

pub type LongMemEvalDataset = Vec<LongMemEvalEntry>;

#[derive(Debug, Serialize, Deserialize)]
pub struct LongMemEvalEntry {
    pub question_id: String,
    pub question_type: String,
    pub question: String,
    pub question_date: String,
    pub answer: LongMemEvalAnswer,
    pub answer_session_ids: Vec<String>,
    pub haystack_dates: Vec<String>,
    pub haystack_session_ids: Vec<String>,
    pub haystack_sessions: Vec<Vec<LongMemEvalMessage>>,
}

impl LongMemEvalEntry {
    pub fn validate(&self) -> anyhow::Result<()> {
        let dates_len = self.haystack_dates.len();
        let session_ids_len = self.haystack_session_ids.len();
        let sessions_len = self.haystack_sessions.len();

        ensure!(
            dates_len == session_ids_len && session_ids_len == sessions_len,
            "LongMemEval entry `{}` has mismatched haystack lengths: dates={}, session_ids={}, sessions={}",
            self.question_id,
            dates_len,
            session_ids_len,
            sessions_len,
        );

        Ok(())
    }

    pub fn sessions(&self) -> anyhow::Result<Vec<LongMemEvalSessionRef<'_>>> {
        self.validate()?;

        Ok(self
            .haystack_dates
            .iter()
            .zip(self.haystack_session_ids.iter())
            .zip(self.haystack_sessions.iter())
            .map(|((date, session_id), messages)| LongMemEvalSessionRef {
                date,
                session_id,
                messages,
            })
            .collect())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LongMemEvalAnswer {
    Text(String),
    Integer(i64),
    Float(f64),
}

impl LongMemEvalAnswer {
    pub fn as_string(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Integer(value) => value.to_string(),
            Self::Float(value) => value.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LongMemEvalSessionRef<'a> {
    pub date: &'a str,
    pub session_id: &'a str,
    pub messages: &'a [LongMemEvalMessage],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LongMemEvalMessage {
    pub role: LongMemEvalRole,
    pub content: String,
    pub has_answer: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LongMemEvalRole {
    User,
    Assistant,
    #[serde(untagged)]
    Other(String),
}

pub fn load_longmemeval_dataset(path: &Path) -> anyhow::Result<LongMemEvalDataset> {
    let json_data = std::fs::read_to_string(path).with_context(|| {
        format!(
            "failed to read LongMemEval dataset file: {}",
            path.display()
        )
    })?;
    let dataset: LongMemEvalDataset =
        serde_json::from_str(&json_data).context("failed to parse LongMemEval dataset JSON")?;

    for entry in &dataset {
        entry.validate().with_context(|| {
            anyhow!(
                "failed to validate LongMemEval dataset entry `{}` from `{}`",
                entry.question_id,
                path.display()
            )
        })?;
    }

    Ok(dataset)
}

pub fn adapt_longmemeval_entry(entry: &LongMemEvalEntry) -> anyhow::Result<BenchmarkCase> {
    let mut sessions = entry
        .sessions()?
        .into_iter()
        .map(|session| Ok((parse_longmemeval_date(session.date)?, session)))
        .collect::<anyhow::Result<Vec<_>>>()?;
    sessions.sort_by(|(left_start, left_session), (right_start, right_session)| {
        left_start
            .cmp(right_start)
            .then(left_session.session_id.cmp(right_session.session_id))
    });

    let case_id = format!("longmemeval:{}", entry.question_id);
    let mut events = Vec::new();
    for (start, session) in &sessions {
        for (turn_idx, message) in session.messages.iter().enumerate() {
            let timestamp = *start + Duration::seconds((turn_idx * 30) as i64);
            events.push(BenchmarkEvent {
                event_id: format!(
                    "longmemeval:{}:{}:t{turn_idx}",
                    entry.question_id, session.session_id
                ),
                stream_id: session.session_id.to_string(),
                timestamp: timestamp.to_rfc3339(),
                content: message.content.clone(),
                speaker_id: Some(longmemeval_role_name(&message.role).to_string()),
                speaker_name: Some(longmemeval_role_name(&message.role).to_string()),
                metadata: json!({
                    "dataset": "longmemeval",
                    "session_id": session.session_id,
                    "session_date": session.date,
                    "has_answer": message.has_answer,
                }),
            });
        }
    }

    let evidence_event_ids = sessions
        .iter()
        .flat_map(|(_, session)| {
            session
                .messages
                .iter()
                .enumerate()
                .filter_map(move |(turn_idx, message)| {
                    message.has_answer.unwrap_or(false).then(|| {
                        format!(
                            "longmemeval:{}:{}:t{turn_idx}",
                            entry.question_id, session.session_id
                        )
                    })
                })
        })
        .collect();

    let question = BenchmarkQuestion {
        question_id: case_id.clone(),
        question: entry.question.clone(),
        question_timestamp: Some(parse_longmemeval_date(&entry.question_date)?.to_rfc3339()),
        gold_answers: vec![entry.answer.as_string()],
        evidence_event_ids,
        evidence_session_ids: entry.answer_session_ids.clone(),
        category: None,
        question_type: Some(entry.question_type.clone()),
        gold_answer_variant: GoldAnswerVariant::Default,
        is_abstention: entry.question_id.ends_with("_abs"),
        metadata: json!({
            "dataset": "longmemeval",
            "raw_question_id": entry.question_id,
            "raw_question_date": entry.question_date,
        }),
    };

    Ok(BenchmarkCase {
        dataset: BenchmarkDataset::LongMemEval,
        case_id,
        events,
        questions: vec![question],
        metadata: json!({
            "dataset": "longmemeval",
            "raw_question_id": entry.question_id,
        }),
    })
}

fn parse_longmemeval_date(value: &str) -> anyhow::Result<DateTime<Utc>> {
    const FORMATS: &[&str] = &[
        "%Y/%m/%d (%a) %H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%d",
    ];

    for format in FORMATS {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(value, format) {
            return Ok(DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc));
        }
    }

    if let Ok(parsed) = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        let parsed = parsed
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow!("invalid date components for `{value}`"))?;
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc));
    }

    if let Ok(parsed) = chrono::NaiveDate::parse_from_str(value, "%Y/%m/%d") {
        let parsed = parsed
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| anyhow!("invalid date components for `{value}`"))?;
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(parsed, Utc));
    }

    anyhow::bail!("failed to parse LongMemEval date `{value}`");
}

fn longmemeval_role_name(role: &LongMemEvalRole) -> &str {
    match role {
        LongMemEvalRole::User => "user",
        LongMemEvalRole::Assistant => "assistant",
        LongMemEvalRole::Other(value) => value.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(content: &str, has_answer: Option<bool>) -> LongMemEvalMessage {
        LongMemEvalMessage {
            role: LongMemEvalRole::User,
            content: content.to_string(),
            has_answer,
        }
    }

    #[test]
    fn validates_matching_parallel_arrays() {
        let entry = LongMemEvalEntry {
            question_id: "q1".to_string(),
            question_type: "multi-session".to_string(),
            question: "question".to_string(),
            question_date: "2024-01-03".to_string(),
            answer: LongMemEvalAnswer::Text("answer".to_string()),
            answer_session_ids: vec!["s1".to_string()],
            haystack_dates: vec!["2024-01-01".to_string()],
            haystack_session_ids: vec!["s1".to_string()],
            haystack_sessions: vec![vec![message("hello", None)]],
        };

        assert!(entry.validate().is_ok());
    }

    #[test]
    fn fails_fast_on_mismatched_parallel_arrays() {
        let entry = LongMemEvalEntry {
            question_id: "q1".to_string(),
            question_type: "multi-session".to_string(),
            question: "question".to_string(),
            question_date: "2024-01-03".to_string(),
            answer: LongMemEvalAnswer::Text("answer".to_string()),
            answer_session_ids: vec!["s1".to_string()],
            haystack_dates: vec!["2024-01-01".to_string()],
            haystack_session_ids: vec!["s1".to_string(), "s2".to_string()],
            haystack_sessions: vec![vec![message("hello", None)]],
        };

        let error = entry.validate().unwrap_err().to_string();
        assert!(error.contains("mismatched haystack lengths"));
    }

    #[test]
    fn adapts_entry_and_preserves_abstention_and_has_answer() {
        let entry = LongMemEvalEntry {
            question_id: "q1_abs".to_string(),
            question_type: "multi-session".to_string(),
            question: "question".to_string(),
            question_date: "2024-01-03".to_string(),
            answer: LongMemEvalAnswer::Text("answer".to_string()),
            answer_session_ids: vec!["s2".to_string()],
            haystack_dates: vec!["2024-01-02".to_string(), "2024-01-01".to_string()],
            haystack_session_ids: vec!["s2".to_string(), "s1".to_string()],
            haystack_sessions: vec![
                vec![message("later", Some(true))],
                vec![message("earlier", None)],
            ],
        };

        let case = adapt_longmemeval_entry(&entry).unwrap();

        assert_eq!(case.case_id, "longmemeval:q1_abs");
        assert_eq!(case.questions[0].question_id, "longmemeval:q1_abs");
        assert!(case.questions[0].is_abstention);
        assert_eq!(
            case.questions[0].evidence_session_ids,
            vec!["s2".to_string()]
        );
        assert_eq!(
            case.questions[0].evidence_event_ids,
            vec!["longmemeval:q1_abs:s2:t0".to_string()]
        );
        assert_eq!(case.events[0].stream_id, "s1");
        assert_eq!(case.events[1].stream_id, "s2");
    }

    #[test]
    fn sorts_sessions_by_parsed_datetime_when_formats_are_mixed() {
        let entry = LongMemEvalEntry {
            question_id: "q2".to_string(),
            question_type: "multi-session".to_string(),
            question: "question".to_string(),
            question_date: "2024-01-03".to_string(),
            answer: LongMemEvalAnswer::Text("answer".to_string()),
            answer_session_ids: vec!["s1".to_string()],
            haystack_dates: vec![
                "2024/01/02 (Tue) 09:00".to_string(),
                "2024-01-01".to_string(),
            ],
            haystack_session_ids: vec!["s2".to_string(), "s1".to_string()],
            haystack_sessions: vec![
                vec![message("later", None)],
                vec![message("earlier", Some(true))],
            ],
        };

        let case = adapt_longmemeval_entry(&entry).unwrap();

        assert_eq!(case.events[0].stream_id, "s1");
        assert_eq!(case.events[1].stream_id, "s2");
        assert!(case.events[0].timestamp < case.events[1].timestamp);
    }
}
