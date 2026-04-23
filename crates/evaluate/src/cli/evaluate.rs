use clap::Parser;
use std::path::PathBuf;

use crate::benchmarks;
use crate::common::{
    answerer::{
        Answerer, DebugAnswerer, LlmBackedAnswerer, LlmBackedAnswererConfig,
        RigOpenAiCompatibleLlmAnswerer,
    },
    backend::{MemoryBackend, ReturnAllMemoryBackend},
    runner::write_outputs,
};
use crate::config::{AnswererConfig, BackendKind, BenchmarkConfig, parse_config_file};

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
    let mut backend = build_backend(run.backend.kind)?;
    let answerer = build_answerer(&run.answerer)?;

    let result = match &run.benchmark {
        BenchmarkConfig::LoCoMo(config) => {
            let prepared =
                benchmarks::locomo_api::prepare_run(&run.input, config, run.judge.as_ref())?;
            benchmarks::execute_prepared_run(prepared, &mut *backend, &*answerer, run.retrieval)
                .await?
        }
        BenchmarkConfig::LongMemEval(config) => {
            let prepared =
                benchmarks::longmemeval_api::prepare_run(&run.input, config, run.judge.as_ref())?;
            benchmarks::execute_prepared_run(prepared, &mut *backend, &*answerer, run.retrieval)
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
                max_retries: 1,
                retry_backoff_ms: 100,
            },
        ));
        assert!(openai.is_ok());
    }
}
