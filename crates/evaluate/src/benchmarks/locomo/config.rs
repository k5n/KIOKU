use anyhow::ensure;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BenchmarkConfig {
    pub(crate) prompt: LocomoKiokuPromptConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocomoKiokuPromptConfig {
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
        prompt: LocomoKiokuPromptConfig {
            answer_template_id: toml.answer_template_id.clone(),
            answer_judge_prompt_id: toml.answer_judge_prompt_id.clone(),
            retrieval_judge_prompt_id: toml.retrieval_judge_prompt_id.clone(),
        },
    }
}

pub(crate) fn validate_config(config: &BenchmarkConfig) -> anyhow::Result<()> {
    ensure!(
        config.prompt.answer_template_id == "locomo.kioku.answer.v1",
        "benchmark.locomo.answer_template_id must be `locomo.kioku.answer.v1`"
    );
    ensure!(
        config.prompt.answer_judge_prompt_id == "locomo.kioku.judge.answer.v1",
        "benchmark.locomo.answer_judge_prompt_id must be `locomo.kioku.judge.answer.v1`"
    );
    ensure!(
        config.prompt.retrieval_judge_prompt_id == "locomo.kioku.judge.retrieval.v1",
        "benchmark.locomo.retrieval_judge_prompt_id must be `locomo.kioku.judge.retrieval.v1`"
    );
    Ok(())
}

pub(crate) fn prompt_config_metadata(config: &BenchmarkConfig) -> LocomoKiokuPromptConfig {
    config.prompt.clone()
}
