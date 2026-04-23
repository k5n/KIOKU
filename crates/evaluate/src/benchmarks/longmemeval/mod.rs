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
    token_counter::WhitespaceTokenCounter,
};
use crate::config::JudgeConfig;

pub(crate) use config::{
    BenchmarkConfig, LongMemEvalKiokuPromptConfig, TomlBenchmarkSection, prompt_config_metadata,
    resolve_config, validate_config,
};
use judge::{LongMemEvalKiokuAnswerJudge, LongMemEvalKiokuRetrievalJudge};
use prompt::LongMemEvalPromptBuilder;
use protocol::LongMemEvalKiokuEvaluationProtocol;

use super::PreparedBenchmarkRun;

pub(crate) fn prepare_run(
    input: &Path,
    config: &BenchmarkConfig,
    judge: Option<&JudgeConfig>,
) -> anyhow::Result<
    PreparedBenchmarkRun<
        LongMemEvalPromptBuilder,
        LongMemEvalKiokuEvaluationProtocol,
        LongMemEvalKiokuAnswerJudge<RigOpenAiCompatibleLlmAnswerer>,
        LongMemEvalKiokuRetrievalJudge<RigOpenAiCompatibleLlmAnswerer>,
    >,
> {
    let cases = dataset::load_dataset(input)?;
    let judge = judge.context("LongMemEval runs require a `[judge]` section")?;
    let (answer_judge, retrieval_judge) = build_judges(judge, &config.prompt)?;

    Ok(PreparedBenchmarkRun {
        cases,
        prompt_builder: LongMemEvalPromptBuilder::new(config.prompt.clone()),
        protocol: LongMemEvalKiokuEvaluationProtocol::new(config.prompt.clone()),
        answer_judge,
        retrieval_judge,
        token_counter: Some(Box::new(WhitespaceTokenCounter)),
    })
}

fn build_judges(
    config: &JudgeConfig,
    prompt: &LongMemEvalKiokuPromptConfig,
) -> anyhow::Result<(
    LongMemEvalKiokuAnswerJudge<RigOpenAiCompatibleLlmAnswerer>,
    LongMemEvalKiokuRetrievalJudge<RigOpenAiCompatibleLlmAnswerer>,
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
                LongMemEvalKiokuAnswerJudge::new(
                    answer_runtime,
                    prompt.answer_judge_prompt_id.clone(),
                ),
                LongMemEvalKiokuRetrievalJudge::new(
                    retrieval_runtime,
                    prompt.retrieval_judge_prompt_id.clone(),
                ),
            ))
        }
    }
}
