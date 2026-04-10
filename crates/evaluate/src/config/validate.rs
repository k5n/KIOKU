use anyhow::{Context, anyhow, ensure};
use std::path::Path;

use crate::model::{BenchmarkDataset, RetrievalBudget};

use super::{
    AnswererConfig, BackendConfig, BackendKind, JudgeConfig, OpenAiCompatibleAnswererConfig,
    OpenAiCompatibleJudgeConfig, PromptConfig, ResolvedConfig, ValidatedConfig,
};

impl ResolvedConfig {
    pub fn validate(self) -> anyhow::Result<ValidatedConfig> {
        validate_backend(
            &self.run.backend,
            &self.run.dataset,
            &self.run.retrieval,
            &self,
        )?;
        validate_prompt(&self.run.prompt, &self.run.dataset, &self)?;
        validate_answerer(&self.run.answerer, &self)?;
        validate_judge(self.run.judge.as_ref(), &self.run.dataset, &self)?;
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
    _dataset: &BenchmarkDataset,
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

fn validate_answerer(answerer: &AnswererConfig, source: &ResolvedConfig) -> anyhow::Result<()> {
    match answerer {
        AnswererConfig::Debug => {
            let _ = source.toml.answerer.debug.as_ref();
            ensure!(
                source.toml.answerer.openai_compatible.is_none(),
                "inactive answerer section `[answerer.openai_compatible]` is not allowed when answerer.kind = \"debug\""
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
            validate_openai_runtime_config(openai, "answerer.openai_compatible")?;
        }
    }

    Ok(())
}

fn validate_judge(
    judge: Option<&JudgeConfig>,
    dataset: &BenchmarkDataset,
    source: &ResolvedConfig,
) -> anyhow::Result<()> {
    match dataset {
        BenchmarkDataset::LoCoMo => {
            let judge = judge.context("LoCoMo runs require a `[judge]` section")?;
            match judge {
                JudgeConfig::OpenAiCompatible(openai) => {
                    let _ = source
                        .toml
                        .judge
                        .as_ref()
                        .and_then(|judge| judge.openai_compatible.as_ref())
                        .context("openai-compatible judge config is missing")?;
                    validate_openai_runtime_config(openai, "judge.openai_compatible")?;
                }
            }
        }
        BenchmarkDataset::LongMemEval => {
            let judge = judge.context("LongMemEval runs require a `[judge]` section")?;
            match judge {
                JudgeConfig::OpenAiCompatible(openai) => {
                    let _ = source
                        .toml
                        .judge
                        .as_ref()
                        .and_then(|judge| judge.openai_compatible.as_ref())
                        .context("openai-compatible judge config is missing")?;
                    validate_openai_runtime_config(openai, "judge.openai_compatible")?;
                }
            }
        }
    }

    Ok(())
}

fn validate_prompt(
    prompt: &PromptConfig,
    dataset: &BenchmarkDataset,
    source: &ResolvedConfig,
) -> anyhow::Result<()> {
    match dataset {
        BenchmarkDataset::LoCoMo => {
            ensure!(
                source
                    .toml
                    .prompt
                    .as_ref()
                    .and_then(|prompt| prompt.longmemeval_kioku.as_ref())
                    .is_none(),
                "inactive prompt section `[prompt.longmemeval_kioku]` is not allowed when run.dataset = \"locomo\""
            );
            let locomo_kioku = prompt.locomo_kioku.as_ref().context(
                "LoCoMo runs require `[prompt.locomo_kioku]` with answer/judge prompt ids",
            )?;
            ensure!(
                locomo_kioku.answer_template_id == "locomo.kioku.answer.v1",
                "prompt.locomo_kioku.answer_template_id must be `locomo.kioku.answer.v1`"
            );
            ensure!(
                locomo_kioku.answer_judge_prompt_id == "locomo.kioku.judge.answer.v1",
                "prompt.locomo_kioku.answer_judge_prompt_id must be `locomo.kioku.judge.answer.v1`"
            );
            ensure!(
                locomo_kioku.retrieval_judge_prompt_id == "locomo.kioku.judge.retrieval.v1",
                "prompt.locomo_kioku.retrieval_judge_prompt_id must be `locomo.kioku.judge.retrieval.v1`"
            );
        }
        BenchmarkDataset::LongMemEval => {
            let longmemeval_kioku = prompt.longmemeval_kioku.as_ref().context(
                "LongMemEval runs require `[prompt.longmemeval_kioku]` with answer/judge prompt ids",
            )?;
            ensure!(
                longmemeval_kioku.answer_template_id == "longmemeval.kioku.answer.v1",
                "prompt.longmemeval_kioku.answer_template_id must be `longmemeval.kioku.answer.v1`"
            );
            ensure!(
                longmemeval_kioku.answer_judge_prompt_id == "longmemeval.kioku.judge.answer.v1",
                "prompt.longmemeval_kioku.answer_judge_prompt_id must be `longmemeval.kioku.judge.answer.v1`"
            );
            ensure!(
                longmemeval_kioku.retrieval_judge_prompt_id
                    == "longmemeval.kioku.judge.retrieval.v1",
                "prompt.longmemeval_kioku.retrieval_judge_prompt_id must be `longmemeval.kioku.judge.retrieval.v1`"
            );
            ensure!(
                source
                    .toml
                    .prompt
                    .as_ref()
                    .and_then(|prompt| prompt.locomo_kioku.as_ref())
                    .is_none(),
                "inactive prompt section `[prompt.locomo_kioku]` is not allowed when run.dataset = \"longmemeval\""
            );
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
    use super::super::{
        AnswererConfig, BackendKind, OpenAiCompatibleAnswererConfig, parse_config_file,
    };
    use crate::config::test_support::write_temp_config;

    #[test]
    fn rejects_inactive_sections_during_validate() {
        let path = write_temp_config(
            "inactive",
            r#"
[run]
dataset = "longmemeval"
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"

[judge]
kind = "openai-compatible"

[judge.openai_compatible]
base_url = "http://localhost:11434/v1"
model = "judge-model"
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 512
timeout_secs = 60
max_retries = 3
retry_backoff_ms = 500

[answerer.openai_compatible]
base_url = "http://localhost:11434/v1"
model = "test"
api_key_env = "OPENAI_API_KEY"
temperature = 0.2
max_output_tokens = 128
timeout_secs = 30
max_retries = 2
retry_backoff_ms = 100

[prompt.longmemeval_kioku]
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
dataset = "longmemeval"
input = "input.json"
output_dir = "{}"

[backend]
kind = "return-all"

[answerer]
kind = "debug"

[judge]
kind = "openai-compatible"

[judge.openai_compatible]
base_url = "http://localhost:11434/v1"
model = "judge-model"
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 512
timeout_secs = 60
max_retries = 3
retry_backoff_ms = 500

[prompt.longmemeval_kioku]
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
    }

    #[test]
    fn keeps_future_kinds_in_schema_but_marks_them_unsupported() {
        let path = write_temp_config(
            "future-kind",
            r#"
[run]
dataset = "locomo"
input = "input.json"
output_dir = "out"

[backend]
kind = "oracle"

[answerer]
kind = "openai-compatible"

[answerer.openai_compatible]
base_url = "http://localhost:11434/v1"
model = "test"
api_key_env = "OPENAI_API_KEY"
temperature = 0.2
max_output_tokens = 128
timeout_secs = 30
max_retries = 2
retry_backoff_ms = 100
"#,
        );

        let resolved = parse_config_file(&path).unwrap().into_resolved().unwrap();
        assert_eq!(resolved.run.backend.kind, BackendKind::Oracle);
        assert_eq!(
            resolved.run.answerer,
            AnswererConfig::OpenAiCompatible(OpenAiCompatibleAnswererConfig {
                base_url: "http://localhost:11434/v1".to_string(),
                model: "test".to_string(),
                api_key_env: "OPENAI_API_KEY".to_string(),
                temperature: 0.2,
                max_output_tokens: 128,
                timeout_secs: 30,
                max_retries: 2,
                retry_backoff_ms: 100,
            })
        );

        let error = resolved.validate().unwrap_err().to_string();
        assert!(error.contains("backend.kind = \"oracle\""));
    }

    #[test]
    fn longmemeval_requires_kioku_prompt_and_judge_settings() {
        let path = write_temp_config(
            "longmemeval-prompt-required",
            r#"
[run]
dataset = "longmemeval"
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"
"#,
        );

        let error = parse_config_file(path)
            .unwrap()
            .into_resolved()
            .unwrap()
            .validate()
            .unwrap_err()
            .to_string();
        assert!(error.contains("[judge]") || error.contains("[prompt.longmemeval_kioku]"));
    }
}
