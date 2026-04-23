use crate::model::{BenchmarkCase, BenchmarkQuestion};

use super::{PreparedPrompt, PromptContext};

#[derive(Debug, Clone, Copy)]
pub struct PromptBuildRequest<'a> {
    pub case: &'a BenchmarkCase,
    pub question: &'a BenchmarkQuestion,
    pub prompt_context: &'a PromptContext,
}

pub trait PromptBuilder: Send + Sync {
    fn build_answer_prompt(
        &self,
        request: PromptBuildRequest<'_>,
    ) -> anyhow::Result<PreparedPrompt>;
}
