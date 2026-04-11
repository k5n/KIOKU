use anyhow::Context;

use crate::model::{BenchmarkDataset, BenchmarkQuestion, MetricsReport};
use crate::prompt::{AnswerPromptProfile, LongMemEvalKiokuPromptConfig};

use super::{DatasetEvaluationProtocol, EvaluatedQuestion};
use crate::runner::ContextTokenPolicy;
use crate::runner::metrics::{LongMemEvalKiokuMetricInput, build_longmemeval_kioku_metrics};

#[derive(Debug, Clone, Copy)]
pub(crate) struct LongMemEvalKiokuEvaluationProtocol<'a> {
    prompt: &'a LongMemEvalKiokuPromptConfig,
}

impl<'a> LongMemEvalKiokuEvaluationProtocol<'a> {
    pub const fn new(prompt: &'a LongMemEvalKiokuPromptConfig) -> Self {
        Self { prompt }
    }
}

impl DatasetEvaluationProtocol for LongMemEvalKiokuEvaluationProtocol<'_> {
    type MetricInput = LongMemEvalKiokuMetricInput;

    fn dataset(&self) -> BenchmarkDataset {
        BenchmarkDataset::LongMemEval
    }

    fn context_token_policy(&self) -> ContextTokenPolicy {
        ContextTokenPolicy::Required
    }

    fn include_question(&self, _question: &BenchmarkQuestion) -> bool {
        true
    }

    fn answer_prompt_profile<'a>(&'a self) -> AnswerPromptProfile<'a> {
        AnswerPromptProfile::LongMemEvalKioku(self.prompt)
    }

    fn build_metric_input(
        &self,
        evaluated: &EvaluatedQuestion<'_>,
    ) -> anyhow::Result<Self::MetricInput> {
        Ok(LongMemEvalKiokuMetricInput {
            question_type: evaluated
                .question
                .question_type
                .clone()
                .context("LongMemEval Kioku metrics require question_type")?,
            is_abstention: evaluated.question.is_abstention,
            answer: evaluated.answer_judgement.clone(),
            retrieval: evaluated.retrieval_judgement.clone(),
            context_token_count: evaluated
                .context_token_count
                .context("LongMemEval Kioku metrics require context_token_count")?,
            answerer_model: evaluated.answerer_model.clone(),
        })
    }

    fn build_metrics(
        &self,
        inputs: &[Self::MetricInput],
        context_tokenizer: Option<&str>,
    ) -> anyhow::Result<MetricsReport> {
        Ok(build_longmemeval_kioku_metrics(
            inputs,
            &self.prompt.answer_judge_prompt_id,
            &self.prompt.retrieval_judge_prompt_id,
            context_tokenizer.context(
                "LongMemEval Kioku metrics require a context_tokenizer provenance value",
            )?,
        ))
    }
}
