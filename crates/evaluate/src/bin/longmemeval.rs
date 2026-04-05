use anyhow::{Context, anyhow, ensure};
use serde::{Deserialize, Serialize};

/// LongMemEval データセット全体を表すルート構造です。
/// JSON のトップレベルは配列で、各要素が 1 問分の評価サンプルです。
pub type LongMemEvalDataset = Vec<LongMemEvalEntry>;

/// LongMemEval の 1 サンプル分のデータです。
/// 質問、正解、正解を含むセッション ID 群、探索対象の会話履歴をまとめて保持します。
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
    /// parallel array の長さが一致していることを検証します。
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

    /// 日付、セッション ID、メッセージ列を同じ添字で束ねて扱いやすくします。
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

/// LongMemEval の回答値です。
/// データセットでは文字列回答と数値回答が混在するため、untagged enum で受けます。
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

/// 1 セッション分の会話履歴への参照です。
/// 元 JSON では日付、セッション ID、メッセージ列が別配列なので、参照で束ねて提供します。
#[derive(Debug, Clone, Copy)]
pub struct LongMemEvalSessionRef<'a> {
    pub date: &'a str,
    pub session_id: &'a str,
    pub messages: &'a [LongMemEvalMessage],
}

/// 会話中の 1 発話を表します。
/// LongMemEval では `user` と `assistant` の 2 種類の role が現れます。
#[derive(Debug, Serialize, Deserialize)]
pub struct LongMemEvalMessage {
    pub role: LongMemEvalRole,
    pub content: String,
    pub has_answer: Option<bool>,
}

/// 発話ロールです。
/// 未知のロールが来ても落とさないように文字列を保持できる形にしています。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LongMemEvalRole {
    User,
    Assistant,
    #[serde(untagged)]
    Other(String),
}

pub fn load_longmemeval_dataset(path: &str) -> anyhow::Result<LongMemEvalDataset> {
    let json_data = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read LongMemEval dataset file: {path}"))?;
    let dataset: LongMemEvalDataset =
        serde_json::from_str(&json_data).context("failed to parse LongMemEval dataset JSON")?;

    for entry in &dataset {
        entry.validate().with_context(|| {
            anyhow!(
                "failed to validate LongMemEval dataset entry `{}` from `{path}`",
                entry.question_id
            )
        })?;
    }

    Ok(dataset)
}

fn main() -> anyhow::Result<()> {
    let path = "data/longmemeval_s_cleaned.json";
    let dataset = load_longmemeval_dataset(path)?;

    for entry in &dataset {
        println!("Question ID: {}", entry.question_id);
        println!("Question Type: {}", entry.question_type);
        println!("Answer: {}", entry.answer.as_string());

        if let Some(session) = entry.sessions()?.first().copied() {
            println!("First Session ID: {}", session.session_id);
            println!("First Session Date: {}", session.date);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(content: &str) -> LongMemEvalMessage {
        LongMemEvalMessage {
            role: LongMemEvalRole::User,
            content: content.to_string(),
            has_answer: None,
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
            haystack_sessions: vec![vec![message("hello")]],
        };

        assert!(entry.validate().is_ok());
        assert_eq!(entry.sessions().unwrap().len(), 1);
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
            haystack_sessions: vec![vec![message("hello")]],
        };

        let error = entry.validate().unwrap_err().to_string();
        assert!(error.contains("mismatched haystack lengths"));
    }
}
