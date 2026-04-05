use async_trait::async_trait;

use crate::judge::{Judge, Judgement};
use crate::model::{BenchmarkQuestion, GeneratedAnswer};

#[derive(Debug, Default)]
pub struct LongMemEvalJudge;

const ABSTENTION_MARKERS: &[&str] = &[
    "unknown",
    "not enough",
    "i don't know",
    "わからない",
    "情報が足りない",
];

#[async_trait]
impl Judge for LongMemEvalJudge {
    async fn judge(
        &self,
        question: &BenchmarkQuestion,
        generated: &GeneratedAnswer,
    ) -> anyhow::Result<Judgement> {
        let normalized_generated = super::traits::normalize_text(&generated.text);
        let exact_match = question
            .gold_answers
            .iter()
            .map(|answer| super::traits::normalize_text(answer))
            .any(|gold| gold == normalized_generated);

        let abstention_match = question.is_abstention
            && ABSTENTION_MARKERS
                .iter()
                .map(|marker| super::traits::normalize_text(marker))
                .any(|marker| normalized_generated.contains(&marker));
        let is_correct = exact_match || abstention_match;

        Ok(Judgement {
            is_correct,
            score: if is_correct { 1.0 } else { 0.0 },
            label: if abstention_match {
                "abstention_match".to_string()
            } else if exact_match {
                "exact_match".to_string()
            } else {
                "mismatch".to_string()
            },
            metadata: super::traits::provisional_metadata("longmemeval_exact_match"),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::judge::{Judge, LoCoMoJudge, LongMemEvalJudge};
    use crate::model::{BenchmarkQuestion, GeneratedAnswer, GoldAnswerVariant};

    fn question() -> BenchmarkQuestion {
        BenchmarkQuestion {
            question_id: "q1".to_string(),
            question: "Q".to_string(),
            question_timestamp: None,
            gold_answers: vec!["New York".to_string()],
            evidence_event_ids: Vec::new(),
            evidence_session_ids: Vec::new(),
            category: None,
            question_type: Some("multi-session".to_string()),
            gold_answer_variant: GoldAnswerVariant::Default,
            is_abstention: false,
            metadata: serde_json::Value::Null,
        }
    }

    #[tokio::test]
    async fn normalized_exact_match_works() {
        let judge = LoCoMoJudge;
        let judgement = judge
            .judge(
                &question(),
                &GeneratedAnswer {
                    text: "  new   york ".to_string(),
                    metadata: serde_json::Value::Null,
                },
            )
            .await
            .unwrap();

        assert!(judgement.is_correct);
    }

    #[tokio::test]
    async fn abstention_marker_counts_for_abstention_questions() {
        let judge = LongMemEvalJudge;
        let mut question = question();
        question.is_abstention = true;
        question.gold_answers = vec!["unanswerable".to_string()];

        let judgement = judge
            .judge(
                &question,
                &GeneratedAnswer {
                    text: "情報が足りないのでわからない".to_string(),
                    metadata: serde_json::Value::Null,
                },
            )
            .await
            .unwrap();

        assert!(judgement.is_correct);
        assert_eq!(judgement.label, "abstention_match");
    }
}
