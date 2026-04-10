use anyhow::Context;
use std::path::{Path, PathBuf};

use crate::model::RetrievalBudget;
use crate::prompt::{LocomoKiokuPromptConfig, LongMemEvalKiokuPromptConfig};

use super::toml::{
    TomlAnswererSection, TomlJudgeSection, TomlOpenAiCompatibleSection, TomlPromptSection,
    TomlRetrievalSection, TomlRunConfig,
};
use super::{
    AnswererConfig, AnswererKind, BackendConfig, JudgeConfig, JudgeKind,
    OpenAiCompatibleAnswererConfig, OpenAiCompatibleJudgeConfig, ParsedConfig, PromptConfig,
    ResolvedConfig, RunConfig,
};

pub fn load_run_config(path: impl AsRef<Path>) -> anyhow::Result<RunConfig> {
    Ok(parse_config_file(path)?.into_resolved()?.validate()?.run)
}

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
            dataset: self.toml.run.dataset,
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
            prompt: resolve_prompt_config(self.toml.prompt.as_ref()),
        };

        Ok(ResolvedConfig {
            source_path,
            raw_bytes: self.raw_bytes,
            toml: self.toml,
            run,
        })
    }
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

// `std::fs::canonicalize` requires the path to exist and resolves symlinks,
// while `std::path::absolute` may keep `..` segments on Unix. We still need a
// lexical normalization step so run metadata records normalized absolute paths
// even for not-yet-created output directories.
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

fn resolve_prompt_config(toml: Option<&TomlPromptSection>) -> PromptConfig {
    PromptConfig {
        longmemeval_kioku: toml.and_then(|prompt| {
            prompt.longmemeval_kioku.as_ref().map(|longmemeval_kioku| {
                LongMemEvalKiokuPromptConfig {
                    answer_template_id: longmemeval_kioku.answer_template_id.clone(),
                    answer_judge_prompt_id: longmemeval_kioku.answer_judge_prompt_id.clone(),
                    retrieval_judge_prompt_id: longmemeval_kioku.retrieval_judge_prompt_id.clone(),
                }
            })
        }),
        locomo_kioku: toml.and_then(|prompt| {
            prompt
                .locomo_kioku
                .as_ref()
                .map(|locomo_kioku| LocomoKiokuPromptConfig {
                    answer_template_id: locomo_kioku.answer_template_id.clone(),
                    answer_judge_prompt_id: locomo_kioku.answer_judge_prompt_id.clone(),
                    retrieval_judge_prompt_id: locomo_kioku.retrieval_judge_prompt_id.clone(),
                })
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_config_file;
    use crate::config::test_support::write_temp_config;
    use crate::prompt::LongMemEvalKiokuPromptConfig;

    #[test]
    fn parses_and_resolves_paths_relative_to_config_file() {
        let path = write_temp_config(
            "resolve",
            r#"
[run]
dataset = "locomo"
input = "../data/input.json"
output_dir = "./out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"
"#,
        );

        let resolved = parse_config_file(&path).unwrap().into_resolved().unwrap();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join(path.parent().unwrap());

        assert_eq!(resolved.run.dataset.as_str(), "locomo");
        assert_eq!(resolved.source_path, std::fs::canonicalize(&path).unwrap());
        assert_eq!(
            resolved.run.input,
            config_dir.parent().unwrap().join("data/input.json")
        );
        assert_eq!(resolved.run.output_dir, config_dir.join("out"));
    }

    #[cfg(unix)]
    #[test]
    fn resolves_paths_relative_to_symlink_location_not_target() {
        use std::os::unix::fs::symlink;

        let temp_root = std::env::temp_dir().join(format!(
            "kioku-evaluate-config-symlink-{}",
            std::process::id()
        ));
        if temp_root.exists() {
            std::fs::remove_dir_all(&temp_root).unwrap();
        }

        let real_dir = temp_root.join("real");
        let link_dir = temp_root.join("configs");
        std::fs::create_dir_all(&real_dir).unwrap();
        std::fs::create_dir_all(&link_dir).unwrap();

        let real_path = real_dir.join("run.toml");
        std::fs::write(
            &real_path,
            r#"
[run]
dataset = "locomo"
input = "./input.json"
output_dir = "./out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"
"#,
        )
        .unwrap();

        let symlink_path = link_dir.join("run.toml");
        symlink(&real_path, &symlink_path).unwrap();

        let resolved = parse_config_file(&symlink_path)
            .unwrap()
            .into_resolved()
            .unwrap();
        let expected_link_dir = std::env::current_dir().unwrap().join(&link_dir);

        assert_eq!(
            resolved.source_path,
            std::fs::canonicalize(&real_path).unwrap()
        );
        assert_eq!(resolved.run.input, expected_link_dir.join("input.json"));
        assert_eq!(resolved.run.output_dir, expected_link_dir.join("out"));

        std::fs::remove_dir_all(temp_root).unwrap();
    }

    #[test]
    fn normalizes_parent_segments_in_resolved_paths() {
        let path = write_temp_config(
            "normalize-parent-segments",
            r#"
[run]
dataset = "locomo"
input = "../data/./input.json"
output_dir = "./nested/../out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"
"#,
        );

        let resolved = parse_config_file(&path).unwrap().into_resolved().unwrap();
        let config_dir = std::env::current_dir()
            .unwrap()
            .join(path.parent().unwrap());

        assert_eq!(
            resolved.run.input,
            config_dir.parent().unwrap().join("data/input.json")
        );
        assert_eq!(resolved.run.output_dir, config_dir.join("out"));
    }

    #[test]
    fn rejects_unknown_field_during_parse() {
        let path = write_temp_config(
            "unknown",
            r#"
[run]
dataset = "locomo"
input = "input.json"
output_dir = "out"
extra = "nope"

[backend]
kind = "return-all"

[answerer]
kind = "debug"
"#,
        );

        let error = parse_config_file(path).unwrap_err();
        let details = format!("{error:#}");
        assert!(details.contains("unknown field"));
    }

    #[test]
    fn rejects_legacy_longmemeval_prompt_section_during_parse() {
        let path = write_temp_config(
            "legacy-longmemeval-prompt",
            r#"
[run]
dataset = "longmemeval"
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[prompt.longmemeval]
answer_profile = "history-chats"
cot = true

[answerer]
kind = "debug"
"#,
        );

        let error = parse_config_file(path).unwrap_err();
        let details = format!("{error:#}");
        assert!(details.contains("unknown field"));
        assert!(details.contains("longmemeval"));
    }

    #[test]
    fn longmemeval_kioku_resolves_protocol_prompt_ids() {
        let path = write_temp_config(
            "longmemeval-kioku-prompt",
            r#"
[run]
dataset = "longmemeval"
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[prompt.longmemeval_kioku]
answer_template_id = "longmemeval.kioku.answer.v1"
answer_judge_prompt_id = "longmemeval.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "longmemeval.kioku.judge.retrieval.v1"

[answerer]
kind = "debug"
"#,
        );

        let resolved = parse_config_file(path).unwrap().into_resolved().unwrap();
        assert_eq!(
            resolved.run.prompt.longmemeval_kioku,
            Some(LongMemEvalKiokuPromptConfig {
                answer_template_id: "longmemeval.kioku.answer.v1".to_string(),
                answer_judge_prompt_id: "longmemeval.kioku.judge.answer.v1".to_string(),
                retrieval_judge_prompt_id: "longmemeval.kioku.judge.retrieval.v1".to_string(),
            })
        );
    }
}
