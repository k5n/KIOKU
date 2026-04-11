mod locomo;
mod longmemeval;

use crate::judge::BinaryJudgement;
use crate::model::{
    BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, GeneratedAnswer, MetricsReport,
};
use crate::prompt::{AnswerPromptProfile, PreparedPrompt, PromptContext};

use super::ContextTokenPolicy;

pub(crate) use locomo::LoCoMoKiokuEvaluationProtocol;
pub(crate) use longmemeval::LongMemEvalKiokuEvaluationProtocol;

pub(crate) trait DatasetEvaluationProtocol {
    type MetricInput;

    fn dataset(&self) -> BenchmarkDataset;
    fn context_token_policy(&self) -> ContextTokenPolicy;
    fn include_question(&self, question: &BenchmarkQuestion) -> bool;
    fn answer_prompt_profile<'a>(&'a self) -> AnswerPromptProfile<'a>;
    fn build_metric_input(
        &self,
        evaluated: &EvaluatedQuestion<'_>,
    ) -> anyhow::Result<Self::MetricInput>;
    fn build_metrics(
        &self,
        inputs: &[Self::MetricInput],
        context_tokenizer: Option<&str>,
    ) -> anyhow::Result<MetricsReport>;
}

#[derive(Debug, Clone)]
pub(crate) struct EvaluatedQuestion<'a> {
    pub dataset: BenchmarkDataset,
    pub case: &'a BenchmarkCase,
    pub question: &'a BenchmarkQuestion,
    pub prompt_context: PromptContext,
    pub query_metadata: serde_json::Value,
    pub retrieval_judgement: BinaryJudgement,
    pub prepared_prompt: PreparedPrompt,
    pub generated_answer: GeneratedAnswer,
    pub answer_judgement: BinaryJudgement,
    pub answerer_model: String,
    pub context_token_count: Option<usize>,
}
