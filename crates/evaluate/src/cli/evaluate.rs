use anyhow::{Context, ensure};
use clap::{Parser, ValueEnum};
use std::path::{Path, PathBuf};

use crate::answerer::DebugAnswerer;
use crate::backend::ReturnAllMemoryBackend;
use crate::datasets::{
    adapt_locomo_entry, adapt_longmemeval_entry, load_locomo_dataset, load_longmemeval_dataset,
};
use crate::judge::{LoCoMoJudge, LongMemEvalJudge};
use crate::model::{BenchmarkCase, BenchmarkDataset, RetrievalBudget};
use crate::runner::{EvaluatePipeline, write_outputs};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DatasetKind {
    Locomo,
    Longmemeval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum BackendKind {
    ReturnAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum AnswererKind {
    Debug,
}

#[derive(Debug, Parser)]
#[command(name = "evaluate")]
pub struct Cli {
    #[arg(long, value_enum)]
    pub dataset: DatasetKind,
    #[arg(long)]
    pub input: PathBuf,
    #[arg(long, value_enum, default_value_t = BackendKind::ReturnAll)]
    backend: BackendKind,
    #[arg(long, value_enum, default_value_t = AnswererKind::Debug)]
    answerer: AnswererKind,
    #[arg(long)]
    pub output_dir: PathBuf,
    #[arg(long)]
    pub max_items: Option<usize>,
    #[arg(long)]
    pub max_tokens: Option<usize>,
}

pub async fn run_cli(cli: Cli) -> anyhow::Result<()> {
    ensure!(
        cli.max_tokens.is_none(),
        "--max-tokens is not supported in Phase 1",
    );
    let budget = RetrievalBudget {
        max_items: cli.max_items,
        max_tokens: cli.max_tokens,
    };

    match (cli.dataset, cli.backend, cli.answerer) {
        (DatasetKind::Locomo, BackendKind::ReturnAll, AnswererKind::Debug) => {
            let cases = load_cases(BenchmarkDataset::LoCoMo, &cli.input)?;
            let mut backend = ReturnAllMemoryBackend::default();
            let answerer = DebugAnswerer::default();
            let judge = LoCoMoJudge;
            let mut pipeline = EvaluatePipeline {
                backend: &mut backend,
                answerer: &answerer,
                judge: &judge,
                budget,
            };
            let result = pipeline.run(&cases).await?;
            write_outputs(&cli.output_dir, &result)
        }
        (DatasetKind::Longmemeval, BackendKind::ReturnAll, AnswererKind::Debug) => {
            let cases = load_cases(BenchmarkDataset::LongMemEval, &cli.input)?;
            let mut backend = ReturnAllMemoryBackend::default();
            let answerer = DebugAnswerer::default();
            let judge = LongMemEvalJudge;
            let mut pipeline = EvaluatePipeline {
                backend: &mut backend,
                answerer: &answerer,
                judge: &judge,
                budget,
            };
            let result = pipeline.run(&cases).await?;
            write_outputs(&cli.output_dir, &result)
        }
    }
}

fn load_cases(dataset: BenchmarkDataset, input: &Path) -> anyhow::Result<Vec<BenchmarkCase>> {
    let input = input
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("input path is not valid UTF-8"))?;

    match dataset {
        BenchmarkDataset::LoCoMo => load_locomo_dataset(input)?
            .iter()
            .map(adapt_locomo_entry)
            .collect::<anyhow::Result<Vec<_>>>()
            .with_context(|| format!("failed to adapt LoCoMo cases from `{input}`")),
        BenchmarkDataset::LongMemEval => load_longmemeval_dataset(input)?
            .iter()
            .map(adapt_longmemeval_entry)
            .collect::<anyhow::Result<Vec<_>>>()
            .with_context(|| format!("failed to adapt LongMemEval cases from `{input}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::{Cli, DatasetKind, run_cli};

    #[tokio::test]
    async fn cli_rejects_max_tokens_in_phase1() {
        let temp = std::env::temp_dir().join("kioku-evaluate-cli-test");
        let result = run_cli(Cli {
            dataset: DatasetKind::Locomo,
            input: temp.join("dummy.json"),
            backend: super::BackendKind::ReturnAll,
            answerer: super::AnswererKind::Debug,
            output_dir: temp.join("out"),
            max_items: None,
            max_tokens: Some(16),
        })
        .await;

        let error = result.unwrap_err().to_string();
        assert!(error.contains("--max-tokens is not supported"));
    }
}
