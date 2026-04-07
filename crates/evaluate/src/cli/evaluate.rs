use anyhow::Context;
use clap::Parser;
use std::path::{Path, PathBuf};

use crate::answerer::{
    Answerer, DebugAnswerer, LlmBackedAnswerer, LlmBackedAnswererConfig,
    RigOpenAiCompatibleLlmAnswerer,
};
use crate::backend::{MemoryBackend, ReturnAllMemoryBackend};
use crate::config::{AnswererConfig, BackendKind, parse_config_file};
use crate::datasets::{
    adapt_locomo_entry, adapt_longmemeval_entry, load_locomo_dataset, load_longmemeval_dataset,
};
use crate::judge::{Judge, LoCoMoJudge, LongMemEvalJudge};
use crate::model::{BenchmarkCase, BenchmarkDataset};
use crate::runner::{EvaluatePipeline, EvaluatePipelineResult, write_outputs};

#[derive(Debug, Parser)]
#[command(name = "evaluate")]
pub struct Cli {
    #[arg(long)]
    pub config: PathBuf,
}

pub async fn run_cli(cli: Cli) -> anyhow::Result<()> {
    let validated = parse_config_file(&cli.config)?
        .into_resolved()?
        .validate()?;
    let run_metadata = validated.resolved_metadata()?;
    let run = validated.run.clone();
    let cases = load_cases(run.dataset, &run.input)?;
    let mut backend = build_backend(run.backend.kind)?;
    let answerer = build_answerer(&run.answerer)?;

    let result = match run.dataset {
        BenchmarkDataset::LoCoMo => {
            run_with_judge(
                &cases,
                &mut *backend,
                &*answerer,
                &LoCoMoJudge,
                run.retrieval,
            )
            .await?
        }
        BenchmarkDataset::LongMemEval => {
            run_with_judge(
                &cases,
                &mut *backend,
                &*answerer,
                &LongMemEvalJudge,
                run.retrieval,
            )
            .await?
        }
    };

    write_outputs(
        &run.output_dir,
        &result,
        &validated.raw_bytes,
        &run_metadata,
    )
}

fn load_cases(dataset: BenchmarkDataset, input: &Path) -> anyhow::Result<Vec<BenchmarkCase>> {
    match dataset {
        BenchmarkDataset::LoCoMo => load_locomo_dataset(input)?
            .iter()
            .map(adapt_locomo_entry)
            .collect::<anyhow::Result<Vec<_>>>()
            .with_context(|| format!("failed to adapt LoCoMo cases from `{}`", input.display())),
        BenchmarkDataset::LongMemEval => load_longmemeval_dataset(input)?
            .iter()
            .map(adapt_longmemeval_entry)
            .collect::<anyhow::Result<Vec<_>>>()
            .with_context(|| {
                format!(
                    "failed to adapt LongMemEval cases from `{}`",
                    input.display()
                )
            }),
    }
}

fn build_backend(kind: BackendKind) -> anyhow::Result<Box<dyn MemoryBackend>> {
    match kind {
        BackendKind::ReturnAll => Ok(Box::new(ReturnAllMemoryBackend::default())),
        BackendKind::Oracle | BackendKind::Kioku => Err(anyhow::anyhow!(
            "unsupported backend.kind: {}",
            kind.as_str()
        )),
    }
}

fn build_answerer(config: &AnswererConfig) -> anyhow::Result<Box<dyn Answerer>> {
    match config {
        AnswererConfig::Debug => Ok(Box::new(DebugAnswerer::default())),
        AnswererConfig::OpenAiCompatible(openai) => {
            let llm = RigOpenAiCompatibleLlmAnswerer::from_answerer_config(openai)?;
            Ok(Box::new(LlmBackedAnswerer::new(
                LlmBackedAnswererConfig {
                    answerer_kind: config.kind().as_str(),
                    temperature: Some(openai.temperature),
                    max_output_tokens: Some(openai.max_output_tokens),
                },
                llm,
            )))
        }
    }
}

async fn run_with_judge<J>(
    cases: &[BenchmarkCase],
    backend: &mut dyn MemoryBackend,
    answerer: &dyn Answerer,
    judge: &J,
    budget: crate::model::RetrievalBudget,
) -> anyhow::Result<EvaluatePipelineResult>
where
    J: Judge,
{
    let mut pipeline = EvaluatePipeline {
        backend,
        answerer,
        judge,
        budget,
    };
    pipeline.run(cases).await
}

#[cfg(test)]
mod tests {
    use super::{Cli, build_answerer};
    use clap::Parser;
    use std::path::PathBuf;

    use crate::config::{AnswererConfig, OpenAiCompatibleAnswererConfig};

    #[test]
    fn cli_requires_config_argument() {
        let result = Cli::try_parse_from(["evaluate"]);
        let error = result.unwrap_err().to_string();
        assert!(error.contains("--config"));
    }

    #[test]
    fn cli_accepts_only_config_argument() {
        let cli = Cli::try_parse_from(["evaluate", "--config", "configs/run.toml"]).unwrap();
        assert_eq!(cli.config, PathBuf::from("configs/run.toml"));
    }

    #[test]
    fn build_answerer_supports_debug_and_openai_compatible_configs() {
        let debug = build_answerer(&AnswererConfig::Debug);
        assert!(debug.is_ok());

        let openai = build_answerer(&AnswererConfig::OpenAiCompatible(
            OpenAiCompatibleAnswererConfig {
                base_url: "http://localhost:11434/v1".to_string(),
                model: "test-model".to_string(),
                api_key_env: "KIOKU_TEST_OPENAI_API_KEY".to_string(),
                temperature: 0.2,
                max_output_tokens: 128,
                timeout_secs: 30,
                max_retries: 2,
                retry_backoff_ms: 10,
            },
        ));
        assert!(openai.is_ok());
    }
}
