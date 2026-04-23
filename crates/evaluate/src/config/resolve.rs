use anyhow::{Context, anyhow};
use std::path::{Path, PathBuf};

use crate::benchmarks::{self, LoCoMoBenchmarkConfig, LongMemEvalBenchmarkConfig};
use crate::model::RetrievalBudget;

use super::toml::{
    TomlAnswererSection, TomlBenchmarkSection, TomlJudgeSection, TomlOpenAiCompatibleSection,
    TomlRetrievalSection, TomlRunConfig,
};
use super::{
    AnswererConfig, AnswererKind, BackendConfig, BenchmarkConfig, JudgeConfig, JudgeKind,
    OpenAiCompatibleAnswererConfig, OpenAiCompatibleJudgeConfig, ParsedConfig, ResolvedConfig,
    RunConfig,
};

pub fn parse_config_file(path: impl AsRef<Path>) -> anyhow::Result<ParsedConfig> {
    let source_path = path.as_ref().to_path_buf();
    let raw_bytes = std::fs::read(&source_path)
        .with_context(|| format!("failed to read config `{}`", source_path.display()))?;
    let raw_text = std::str::from_utf8(&raw_bytes)
        .with_context(|| format!("config `{}` is not valid UTF-8", source_path.display()))?;
    let toml = ::toml::from_str::<TomlRunConfig>(raw_text)
        .with_context(|| format!("failed to parse TOML config `{}`", source_path.display()))?;

    Ok(ParsedConfig {
        source_path,
        raw_bytes,
        toml,
    })
}

impl ParsedConfig {
    pub fn into_resolved(self) -> anyhow::Result<ResolvedConfig> {
        let config_dir = absolute_path(self.source_path.parent().unwrap_or_else(|| Path::new(".")))
            .with_context(|| {
                format!(
                    "failed to resolve config directory for `{}`",
                    self.source_path.display()
                )
            })?;
        let source_path = std::fs::canonicalize(&self.source_path).with_context(|| {
            format!(
                "failed to canonicalize config path `{}`",
                self.source_path.display()
            )
        })?;
        let retrieval = self.toml.retrieval.clone().unwrap_or(TomlRetrievalSection {
            max_items: None,
            max_tokens: None,
        });

        let run = RunConfig {
            input: resolve_path(&config_dir, &self.toml.run.input),
            output_dir: resolve_path(&config_dir, &self.toml.run.output_dir),
            backend: BackendConfig {
                kind: self.toml.backend.kind,
            },
            answerer: resolve_answerer_config(&self.toml.answerer)?,
            judge: resolve_judge_config(self.toml.judge.as_ref())?,
            retrieval: RetrievalBudget {
                max_items: retrieval.max_items,
                max_tokens: retrieval.max_tokens,
            },
            benchmark: resolve_benchmark_config(self.toml.benchmark.as_ref())?,
        };

        Ok(ResolvedConfig {
            source_path,
            raw_bytes: self.raw_bytes,
            toml: self.toml,
            run,
        })
    }
}

fn resolve_benchmark_config(
    toml: Option<&TomlBenchmarkSection>,
) -> anyhow::Result<BenchmarkConfig> {
    let Some(toml) = toml else {
        return Err(anyhow!(
            "config must contain exactly one benchmark section: `[benchmark.locomo]` or `[benchmark.longmemeval]`"
        ));
    };

    match (&toml.locomo, &toml.longmemeval) {
        (Some(locomo), None) => Ok(BenchmarkConfig::LoCoMo(resolve_locomo_config(locomo))),
        (None, Some(longmemeval)) => Ok(BenchmarkConfig::LongMemEval(resolve_longmemeval_config(
            longmemeval,
        ))),
        (None, None) => Err(anyhow!(
            "config must contain exactly one benchmark section: `[benchmark.locomo]` or `[benchmark.longmemeval]`"
        )),
        (Some(_), Some(_)) => Err(anyhow!(
            "config must contain exactly one benchmark section, but both `[benchmark.locomo]` and `[benchmark.longmemeval]` were provided"
        )),
    }
}

fn resolve_locomo_config(toml: &benchmarks::TomlLoCoMoBenchmarkSection) -> LoCoMoBenchmarkConfig {
    benchmarks::locomo_api::resolve_config(toml)
}

fn resolve_longmemeval_config(
    toml: &benchmarks::TomlLongMemEvalBenchmarkSection,
) -> LongMemEvalBenchmarkConfig {
    benchmarks::longmemeval_api::resolve_config(toml)
}

fn resolve_path(base_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize_path(path)
    } else {
        normalize_path(&base_dir.join(path))
    }
}

fn absolute_path(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(normalize_path(path))
    } else {
        Ok(normalize_path(&std::env::current_dir()?.join(path)))
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = normalized
                    .components()
                    .next_back()
                    .is_some_and(|last| !matches!(last, Component::RootDir | Component::Prefix(_)));

                if can_pop {
                    normalized.pop();
                } else if !path.is_absolute() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }

    if normalized.as_os_str().is_empty() {
        if path.is_absolute() {
            PathBuf::from(std::path::MAIN_SEPARATOR.to_string())
        } else {
            PathBuf::from(".")
        }
    } else {
        normalized
    }
}

fn resolve_answerer_config(toml: &TomlAnswererSection) -> anyhow::Result<AnswererConfig> {
    match toml.kind {
        AnswererKind::Debug => Ok(AnswererConfig::Debug),
        AnswererKind::OpenAiCompatible => {
            let openai = toml
                .openai_compatible
                .as_ref()
                .context("openai-compatible answerer config is missing")?;
            Ok(AnswererConfig::OpenAiCompatible(
                resolve_openai_compatible_answerer_config(openai),
            ))
        }
    }
}

fn resolve_openai_compatible_answerer_config(
    openai: &TomlOpenAiCompatibleSection,
) -> OpenAiCompatibleAnswererConfig {
    OpenAiCompatibleAnswererConfig {
        base_url: openai.base_url.clone(),
        model: openai.model.clone(),
        api_key_env: openai.api_key_env.clone(),
        temperature: openai.temperature,
        max_output_tokens: openai.max_output_tokens,
        timeout_secs: openai.timeout_secs,
        max_retries: openai.max_retries,
        retry_backoff_ms: openai.retry_backoff_ms,
    }
}

fn resolve_judge_config(toml: Option<&TomlJudgeSection>) -> anyhow::Result<Option<JudgeConfig>> {
    let Some(toml) = toml else {
        return Ok(None);
    };

    match toml.kind {
        JudgeKind::OpenAiCompatible => {
            let openai = toml
                .openai_compatible
                .as_ref()
                .context("openai-compatible judge config is missing")?;
            Ok(Some(JudgeConfig::OpenAiCompatible(
                resolve_openai_compatible_judge_config(openai),
            )))
        }
    }
}

fn resolve_openai_compatible_judge_config(
    openai: &TomlOpenAiCompatibleSection,
) -> OpenAiCompatibleJudgeConfig {
    OpenAiCompatibleJudgeConfig {
        base_url: openai.base_url.clone(),
        model: openai.model.clone(),
        api_key_env: openai.api_key_env.clone(),
        temperature: openai.temperature,
        max_output_tokens: openai.max_output_tokens,
        timeout_secs: openai.timeout_secs,
        max_retries: openai.max_retries,
        retry_backoff_ms: openai.retry_backoff_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_config_file;
    use crate::config::test_support::write_temp_config;
    use crate::model::BenchmarkDataset;

    #[test]
    fn parses_and_resolves_paths_relative_to_config_file() {
        let path = write_temp_config(
            "resolve",
            r#"
[run]
input = "../data/input.json"
output_dir = "./out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"

[benchmark.locomo]
answer_template_id = "locomo.kioku.answer.v1"
answer_judge_prompt_id = "locomo.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "locomo.kioku.judge.retrieval.v1"
"#,
        );

        let resolved = parse_config_file(&path).unwrap().into_resolved().unwrap();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join(path.parent().unwrap());

        assert_eq!(resolved.run.dataset(), BenchmarkDataset::LoCoMo);
        assert_eq!(resolved.source_path, std::fs::canonicalize(&path).unwrap());
        assert_eq!(
            resolved.run.input,
            config_dir.parent().unwrap().join("data/input.json")
        );
        assert_eq!(resolved.run.output_dir, config_dir.join("out"));
    }

    #[test]
    fn rejects_missing_or_multiple_benchmark_sections() {
        let missing = write_temp_config(
            "missing-benchmark",
            r#"
[run]
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"
"#,
        );
        let error = parse_config_file(&missing)
            .unwrap()
            .into_resolved()
            .unwrap_err()
            .to_string();
        assert!(error.contains("exactly one benchmark section"));

        let multiple = write_temp_config(
            "multiple-benchmark",
            r#"
[run]
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"

[benchmark.locomo]
answer_template_id = "locomo.kioku.answer.v1"
answer_judge_prompt_id = "locomo.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "locomo.kioku.judge.retrieval.v1"

[benchmark.longmemeval]
answer_template_id = "longmemeval.kioku.answer.v1"
answer_judge_prompt_id = "longmemeval.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "longmemeval.kioku.judge.retrieval.v1"
"#,
        );
        let error = parse_config_file(&multiple)
            .unwrap()
            .into_resolved()
            .unwrap_err()
            .to_string();
        assert!(error.contains("both `[benchmark.locomo]` and `[benchmark.longmemeval]`"));
    }
}
