use anyhow::{Context, anyhow, ensure};
use std::path::Path;

use crate::benchmarks;
use crate::model::RetrievalBudget;

use super::{
    AnswererConfig, BackendConfig, BackendKind, BenchmarkConfig, JudgeConfig,
    OpenAiCompatibleAnswererConfig, OpenAiCompatibleJudgeConfig, ResolvedConfig, ValidatedConfig,
};

impl ResolvedConfig {
    pub fn validate(self) -> anyhow::Result<ValidatedConfig> {
        validate_backend(&self.run.backend, &self.run.retrieval, &self)?;
        validate_benchmark(&self.run.benchmark)?;
        validate_answerer(&self.run.answerer, &self)?;
        validate_judge(self.run.judge.as_ref(), &self.run.benchmark, &self)?;
        validate_output_dir(&self.run.output_dir)?;
        Ok(ValidatedConfig {
            source_path: self.source_path,
            raw_bytes: self.raw_bytes,
            run: self.run,
        })
    }
}

fn validate_backend(
    backend: &BackendConfig,
    retrieval: &RetrievalBudget,
    source: &ResolvedConfig,
) -> anyhow::Result<()> {
    match backend.kind {
        BackendKind::ReturnAll => {
            ensure!(
                source.toml.backend.oracle.is_none(),
                "inactive backend section `[backend.oracle]` is not allowed when backend.kind = \"{}\"",
                backend.kind.as_str()
            );
            ensure!(
                source.toml.backend.kioku.is_none(),
                "inactive backend section `[backend.kioku]` is not allowed when backend.kind = \"{}\"",
                backend.kind.as_str()
            );
            let _ = source.toml.backend.return_all.as_ref();
            ensure!(
                retrieval.max_tokens.is_none(),
                "max_tokens is not supported by return-all backend in Phase 2"
            );
        }
        BackendKind::Oracle | BackendKind::Kioku => {
            return Err(anyhow!(
                "backend.kind = \"{}\" is not supported in Phase 2",
                backend.kind.as_str()
            ));
        }
    }

    Ok(())
}

fn validate_benchmark(benchmark: &BenchmarkConfig) -> anyhow::Result<()> {
    match benchmark {
        BenchmarkConfig::LoCoMo(config) => benchmarks::locomo_api::validate_config(config),
        BenchmarkConfig::LongMemEval(config) => {
            benchmarks::longmemeval_api::validate_config(config)
        }
    }
}

fn validate_answerer(answerer: &AnswererConfig, source: &ResolvedConfig) -> anyhow::Result<()> {
    match answerer {
        AnswererConfig::Debug => {
            let _ = source.toml.answerer.debug.as_ref();
            ensure!(
                source.toml.answerer.openai_compatible.is_none(),
                "inactive answerer section `[answerer.openai-compatible]` is not allowed when answerer.kind = \"debug\""
            );
        }
        AnswererConfig::OpenAiCompatible(openai) => {
            ensure!(
                source.toml.answerer.debug.is_none(),
                "inactive answerer section `[answerer.debug]` is not allowed when answerer.kind = \"openai-compatible\""
            );
            let _ = source
                .toml
                .answerer
                .openai_compatible
                .as_ref()
                .context("openai-compatible answerer config is missing")?;
            validate_openai_runtime_config(openai, "answerer.openai-compatible")?;
        }
    }

    Ok(())
}

fn validate_judge(
    judge: Option<&JudgeConfig>,
    benchmark: &BenchmarkConfig,
    source: &ResolvedConfig,
) -> anyhow::Result<()> {
    let judge = match benchmark {
        BenchmarkConfig::LoCoMo(_) => judge.context("LoCoMo runs require a `[judge]` section")?,
        BenchmarkConfig::LongMemEval(_) => {
            judge.context("LongMemEval runs require a `[judge]` section")?
        }
    };

    match judge {
        JudgeConfig::OpenAiCompatible(openai) => {
            let _ = source
                .toml
                .judge
                .as_ref()
                .and_then(|judge| judge.openai_compatible.as_ref())
                .context("openai-compatible judge config is missing")?;
            validate_openai_runtime_config(openai, "judge.openai-compatible")?;
        }
    }

    Ok(())
}

fn validate_openai_runtime_config(
    openai: &impl OpenAiRuntimeConfig,
    field_prefix: &str,
) -> anyhow::Result<()> {
    ensure!(
        !openai.base_url().trim().is_empty(),
        "{field_prefix}.base_url must not be empty"
    );
    ensure!(
        !openai.model().trim().is_empty(),
        "{field_prefix}.model must not be empty"
    );
    ensure!(
        !openai.api_key_env().trim().is_empty(),
        "{field_prefix}.api_key_env must not be empty"
    );
    ensure!(
        openai.timeout_secs() > 0,
        "{field_prefix}.timeout_secs must be greater than 0"
    );
    ensure!(
        openai.max_output_tokens() > 0,
        "{field_prefix}.max_output_tokens must be greater than 0"
    );
    Ok(())
}

trait OpenAiRuntimeConfig {
    fn base_url(&self) -> &str;
    fn model(&self) -> &str;
    fn api_key_env(&self) -> &str;
    fn timeout_secs(&self) -> u64;
    fn max_output_tokens(&self) -> u32;
}

impl OpenAiRuntimeConfig for OpenAiCompatibleAnswererConfig {
    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn api_key_env(&self) -> &str {
        &self.api_key_env
    }

    fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    fn max_output_tokens(&self) -> u32 {
        self.max_output_tokens
    }
}

impl OpenAiRuntimeConfig for OpenAiCompatibleJudgeConfig {
    fn base_url(&self) -> &str {
        &self.base_url
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn api_key_env(&self) -> &str {
        &self.api_key_env
    }

    fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    fn max_output_tokens(&self) -> u32 {
        self.max_output_tokens
    }
}

fn validate_output_dir(output_dir: &Path) -> anyhow::Result<()> {
    if !output_dir.exists() {
        return Ok(());
    }

    ensure!(
        output_dir.is_dir(),
        "output_dir `{}` exists and is not a directory",
        output_dir.display()
    );

    let mut entries = std::fs::read_dir(output_dir)
        .with_context(|| format!("failed to read output_dir `{}`", output_dir.display()))?;
    ensure!(
        entries.next().transpose()?.is_none(),
        "output_dir `{}` already exists and is not empty",
        output_dir.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::parse_config_file;
    use crate::config::test_support::write_temp_config;

    #[test]
    fn rejects_unsupported_backend_and_missing_judge() {
        let unsupported = write_temp_config(
            "unsupported-backend",
            r#"
[run]
input = "input.json"
output_dir = "out"

[backend]
kind = "oracle"

[answerer]
kind = "debug"

[benchmark.locomo]
answer_template_id = "locomo.kioku.answer.v1"
answer_judge_prompt_id = "locomo.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "locomo.kioku.judge.retrieval.v1"
"#,
        );
        let error = parse_config_file(&unsupported)
            .unwrap()
            .into_resolved()
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string();
        assert!(error.contains("not supported"));

        let missing_judge = write_temp_config(
            "missing-judge",
            r#"
[run]
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"

[benchmark.longmemeval]
answer_template_id = "longmemeval.kioku.answer.v1"
answer_judge_prompt_id = "longmemeval.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "longmemeval.kioku.judge.retrieval.v1"
"#,
        );
        let error = parse_config_file(&missing_judge)
            .unwrap()
            .into_resolved()
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string();
        assert!(error.contains("[judge]"));
    }

    #[test]
    fn validates_benchmark_specific_prompt_ids() {
        let path = write_temp_config(
            "invalid-benchmark",
            r#"
[run]
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"

[judge]
kind = "openai-compatible"

[judge.openai-compatible]
base_url = "http://localhost:11434/v1"
model = "judge-model"
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 512
timeout_secs = 60
max_retries = 3
retry_backoff_ms = 500

[benchmark.locomo]
answer_template_id = "wrong"
answer_judge_prompt_id = "locomo.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "locomo.kioku.judge.retrieval.v1"
"#,
        );
        let error = parse_config_file(&path)
            .unwrap()
            .into_resolved()
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string();
        assert!(error.contains("benchmark.locomo.answer_template_id"));
    }

    #[test]
    fn rejects_inactive_sections_during_validate() {
        let path = write_temp_config(
            "inactive",
            r#"
[run]
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"

[judge]
kind = "openai-compatible"

[judge.openai-compatible]
base_url = "http://localhost:11434/v1"
model = "judge-model"
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 512
timeout_secs = 60
max_retries = 3
retry_backoff_ms = 500

[answerer.openai-compatible]
base_url = "http://localhost:11434/v1"
model = "test"
api_key_env = "OPENAI_API_KEY"
temperature = 0.2
max_output_tokens = 128
timeout_secs = 30
max_retries = 2
retry_backoff_ms = 100

[benchmark.longmemeval]
answer_template_id = "longmemeval.kioku.answer.v1"
answer_judge_prompt_id = "longmemeval.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "longmemeval.kioku.judge.retrieval.v1"
"#,
        );

        let error = parse_config_file(path)
            .unwrap()
            .into_resolved()
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string();
        assert!(error.contains("inactive answerer section"));
    }

    #[test]
    fn rejects_non_empty_output_dir() {
        let dir = std::env::temp_dir().join(format!(
            "kioku-evaluate-config-output-{}",
            std::process::id()
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("existing.txt"), "x").unwrap();

        let path = write_temp_config(
            "output",
            &format!(
                r#"
[run]
input = "input.json"
output_dir = "{}"

[backend]
kind = "return-all"

[answerer]
kind = "debug"

[judge]
kind = "openai-compatible"

[judge.openai-compatible]
base_url = "http://localhost:11434/v1"
model = "judge-model"
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 512
timeout_secs = 60
max_retries = 3
retry_backoff_ms = 500

[benchmark.longmemeval]
answer_template_id = "longmemeval.kioku.answer.v1"
answer_judge_prompt_id = "longmemeval.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "longmemeval.kioku.judge.retrieval.v1"
"#,
                dir.display()
            ),
        );

        let error = parse_config_file(path)
            .unwrap()
            .into_resolved()
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string();
        assert!(error.contains("already exists and is not empty"));

        std::fs::remove_dir_all(dir).unwrap();
    }
}
