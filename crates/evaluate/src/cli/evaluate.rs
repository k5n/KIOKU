use anyhow::Context;
use clap::Parser;
use std::path::{Path, PathBuf};

use crate::answerer::DebugAnswerer;
use crate::backend::ReturnAllMemoryBackend;
use crate::config::{AnswererKind, BackendKind, parse_config_file};
use crate::datasets::{
    adapt_locomo_entry, adapt_longmemeval_entry, load_locomo_dataset, load_longmemeval_dataset,
};
use crate::judge::{LoCoMoJudge, LongMemEvalJudge};
use crate::model::{BenchmarkCase, BenchmarkDataset};
use crate::runner::{EvaluatePipeline, write_outputs};

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

    match (run.dataset, run.backend.kind, run.answerer.kind) {
        (BenchmarkDataset::LoCoMo, BackendKind::ReturnAll, AnswererKind::Debug) => {
            let cases = load_cases(BenchmarkDataset::LoCoMo, &run.input)?;
            let mut backend = ReturnAllMemoryBackend::default();
            let answerer = DebugAnswerer::default();
            let judge = LoCoMoJudge;
            let mut pipeline = EvaluatePipeline {
                backend: &mut backend,
                answerer: &answerer,
                judge: &judge,
                budget: run.retrieval,
            };
            let result = pipeline.run(&cases).await?;
            write_outputs(
                &run.output_dir,
                &result,
                &validated.raw_bytes,
                &run_metadata,
            )
        }
        (BenchmarkDataset::LongMemEval, BackendKind::ReturnAll, AnswererKind::Debug) => {
            let cases = load_cases(BenchmarkDataset::LongMemEval, &run.input)?;
            let mut backend = ReturnAllMemoryBackend::default();
            let answerer = DebugAnswerer::default();
            let judge = LongMemEvalJudge;
            let mut pipeline = EvaluatePipeline {
                backend: &mut backend,
                answerer: &answerer,
                judge: &judge,
                budget: run.retrieval,
            };
            let result = pipeline.run(&cases).await?;
            write_outputs(
                &run.output_dir,
                &result,
                &validated.raw_bytes,
                &run_metadata,
            )
        }
        (dataset, backend, answerer) => Err(anyhow::anyhow!(
            "unsupported run combination: dataset={}, backend={}, answerer={}",
            dataset.as_str(),
            backend.as_str(),
            answerer.as_str()
        )),
    }
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

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;
    use std::path::PathBuf;

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
}
