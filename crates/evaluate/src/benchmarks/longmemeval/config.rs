use anyhow::ensure;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BenchmarkConfig {
    pub(crate) prompt: LongMemEvalKiokuPromptConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongMemEvalKiokuPromptConfig {
    pub answer_template_id: String,
    pub answer_judge_prompt_id: String,
    pub retrieval_judge_prompt_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TomlBenchmarkSection {
    pub(crate) answer_template_id: String,
    pub(crate) answer_judge_prompt_id: String,
    pub(crate) retrieval_judge_prompt_id: String,
}

pub(crate) fn resolve_config(toml: &TomlBenchmarkSection) -> BenchmarkConfig {
    BenchmarkConfig {
        prompt: LongMemEvalKiokuPromptConfig {
            answer_template_id: toml.answer_template_id.clone(),
            answer_judge_prompt_id: toml.answer_judge_prompt_id.clone(),
            retrieval_judge_prompt_id: toml.retrieval_judge_prompt_id.clone(),
        },
    }
}

pub(crate) fn validate_config(config: &BenchmarkConfig) -> anyhow::Result<()> {
    ensure!(
        config.prompt.answer_template_id == "longmemeval.kioku.answer.v1",
        "benchmark.longmemeval.answer_template_id must be `longmemeval.kioku.answer.v1`"
    );
    ensure!(
        config.prompt.answer_judge_prompt_id == "longmemeval.kioku.judge.answer.v1",
        "benchmark.longmemeval.answer_judge_prompt_id must be `longmemeval.kioku.judge.answer.v1`"
    );
    ensure!(
        config.prompt.retrieval_judge_prompt_id == "longmemeval.kioku.judge.retrieval.v1",
        "benchmark.longmemeval.retrieval_judge_prompt_id must be `longmemeval.kioku.judge.retrieval.v1`"
    );
    Ok(())
}

pub(crate) fn prompt_config_metadata(config: &BenchmarkConfig) -> LongMemEvalKiokuPromptConfig {
    config.prompt.clone()
}
