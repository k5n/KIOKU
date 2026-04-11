use anyhow::Context;

use crate::model::{BenchmarkDataset, BenchmarkQuestion, MetricsReport};
use crate::prompt::{AnswerPromptProfile, LocomoKiokuPromptConfig};

use super::{DatasetEvaluationProtocol, EvaluatedQuestion};
use crate::runner::ContextTokenPolicy;
use crate::runner::metrics::{LoCoMoKiokuMetricInput, build_locomo_kioku_metrics};

#[derive(Debug, Clone, Copy)]
pub(crate) struct LoCoMoKiokuEvaluationProtocol<'a> {
    prompt: &'a LocomoKiokuPromptConfig,
}

impl<'a> LoCoMoKiokuEvaluationProtocol<'a> {
    pub const fn new(prompt: &'a LocomoKiokuPromptConfig) -> Self {
        Self { prompt }
    }
}

impl DatasetEvaluationProtocol for LoCoMoKiokuEvaluationProtocol<'_> {
    type MetricInput = LoCoMoKiokuMetricInput;

    fn dataset(&self) -> BenchmarkDataset {
        BenchmarkDataset::LoCoMo
    }

    fn context_token_policy(&self) -> ContextTokenPolicy {
        ContextTokenPolicy::Optional
    }

    fn include_question(&self, question: &BenchmarkQuestion) -> bool {
        matches!(question.category, Some(1..=4))
    }

    fn answer_prompt_profile<'a>(&'a self) -> AnswerPromptProfile<'a> {
        AnswerPromptProfile::LoCoMoKioku(self.prompt)
    }

    fn build_metric_input(
        &self,
        evaluated: &EvaluatedQuestion<'_>,
    ) -> anyhow::Result<Self::MetricInput> {
        Ok(LoCoMoKiokuMetricInput {
            category: evaluated
                .question
                .category
                .context("LoCoMo Kioku metrics require category after protocol filtering")?,
            answer: evaluated.answer_judgement.clone(),
            retrieval: evaluated.retrieval_judgement.clone(),
            answerer_model: evaluated.answerer_model.clone(),
        })
    }

    fn build_metrics(
        &self,
        inputs: &[Self::MetricInput],
        _context_tokenizer: Option<&str>,
    ) -> anyhow::Result<MetricsReport> {
        Ok(build_locomo_kioku_metrics(
            inputs,
            &self.prompt.answer_judge_prompt_id,
            &self.prompt.retrieval_judge_prompt_id,
        ))
    }
}
