use async_trait::async_trait;

use crate::judge::{Judge, Judgement};
use crate::model::{BenchmarkQuestion, GeneratedAnswer};

#[derive(Debug, Default)]
pub struct LoCoMoJudge;

#[async_trait]
impl Judge for LoCoMoJudge {
    async fn judge(
        &self,
        question: &BenchmarkQuestion,
        generated: &GeneratedAnswer,
    ) -> anyhow::Result<Judgement> {
        let normalized_generated = super::traits::normalize_text(&generated.text);
        let is_correct = question
            .gold_answers
            .iter()
            .map(|answer| super::traits::normalize_text(answer))
            .any(|gold| gold == normalized_generated);

        Ok(Judgement {
            is_correct,
            score: if is_correct { 1.0 } else { 0.0 },
            label: if is_correct {
                "exact_match".to_string()
            } else {
                "mismatch".to_string()
            },
            metadata: super::traits::provisional_metadata("locomo_exact_match"),
        })
    }
}
