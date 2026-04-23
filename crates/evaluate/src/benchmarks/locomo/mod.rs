mod config;
mod dataset;
mod judge;
mod metrics;
mod prompt;
mod protocol;

use std::path::Path;

use anyhow::Context;

use crate::common::{
    answerer::{RigOpenAiCompatibleConfig, RigOpenAiCompatibleLlmAnswerer},
    judge::OpenAiCompatibleJudgeRuntime,
};
use crate::config::JudgeConfig;

pub(crate) use config::{
    BenchmarkConfig, LocomoKiokuPromptConfig, TomlBenchmarkSection, prompt_config_metadata,
    resolve_config, validate_config,
};
use judge::{LoCoMoKiokuAnswerJudge, LoCoMoKiokuRetrievalJudge};
use prompt::LocomoPromptBuilder;
use protocol::LoCoMoKiokuEvaluationProtocol;

use super::PreparedBenchmarkRun;

pub(crate) fn prepare_run(
    input: &Path,
    config: &BenchmarkConfig,
    judge: Option<&JudgeConfig>,
) -> anyhow::Result<
    PreparedBenchmarkRun<
        LocomoPromptBuilder,
        LoCoMoKiokuEvaluationProtocol,
        LoCoMoKiokuAnswerJudge<RigOpenAiCompatibleLlmAnswerer>,
        LoCoMoKiokuRetrievalJudge<RigOpenAiCompatibleLlmAnswerer>,
    >,
> {
    let cases = dataset::load_dataset(input)?;
    let judge = judge.context("LoCoMo runs require a `[judge]` section")?;
    let (answer_judge, retrieval_judge) = build_judges(judge, &config.prompt)?;

    Ok(PreparedBenchmarkRun {
        cases,
        prompt_builder: LocomoPromptBuilder::new(config.prompt.clone()),
        protocol: LoCoMoKiokuEvaluationProtocol::new(config.prompt.clone()),
        answer_judge,
        retrieval_judge,
        token_counter: None,
    })
}

fn build_judges(
    config: &JudgeConfig,
    prompt: &LocomoKiokuPromptConfig,
) -> anyhow::Result<(
    LoCoMoKiokuAnswerJudge<RigOpenAiCompatibleLlmAnswerer>,
    LoCoMoKiokuRetrievalJudge<RigOpenAiCompatibleLlmAnswerer>,
)> {
    match config {
        JudgeConfig::OpenAiCompatible(openai) => {
            let answer_runtime = OpenAiCompatibleJudgeRuntime::new(
                RigOpenAiCompatibleLlmAnswerer::new(RigOpenAiCompatibleConfig::from_judge_config(
                    openai,
                ))?,
                openai.model.clone(),
                Some(openai.temperature),
                Some(openai.max_output_tokens),
            );
            let retrieval_runtime = OpenAiCompatibleJudgeRuntime::new(
                RigOpenAiCompatibleLlmAnswerer::new(RigOpenAiCompatibleConfig::from_judge_config(
                    openai,
                ))?,
                openai.model.clone(),
                Some(openai.temperature),
                Some(openai.max_output_tokens),
            );

            Ok((
                LoCoMoKiokuAnswerJudge::new(answer_runtime, prompt.answer_judge_prompt_id.clone()),
                LoCoMoKiokuRetrievalJudge::new(
                    retrieval_runtime,
                    prompt.retrieval_judge_prompt_id.clone(),
                ),
            ))
        }
    }
}
