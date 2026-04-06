use anyhow::{Context, anyhow, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::model::{BenchmarkDataset, RetrievalBudget};

#[derive(Debug, Clone)]
pub struct ParsedConfig {
    pub source_path: PathBuf,
    pub raw_bytes: Vec<u8>,
    toml: TomlRunConfig,
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub source_path: PathBuf,
    pub raw_bytes: Vec<u8>,
    toml: TomlRunConfig,
    pub run: RunConfig,
}

#[derive(Debug, Clone)]
pub struct ValidatedConfig {
    pub source_path: PathBuf,
    pub raw_bytes: Vec<u8>,
    pub run: RunConfig,
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub dataset: BenchmarkDataset,
    pub input: PathBuf,
    pub output_dir: PathBuf,
    pub backend: BackendConfig,
    pub answerer: AnswererConfig,
    pub retrieval: RetrievalBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendConfig {
    pub kind: BackendKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendKind {
    ReturnAll,
    Oracle,
    Kioku,
}

impl BackendKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReturnAll => "return-all",
            Self::Oracle => "oracle",
            Self::Kioku => "kioku",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnswererConfig {
    Debug,
    OpenAiCompatible(OpenAiCompatibleAnswererConfig),
}

impl AnswererConfig {
    pub fn kind(&self) -> AnswererKind {
        match self {
            Self::Debug => AnswererKind::Debug,
            Self::OpenAiCompatible(_) => AnswererKind::OpenAiCompatible,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnswererKind {
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

impl AnswererKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenAiCompatibleAnswererConfig {
    pub base_url: String,
    pub model: String,
    pub api_key_env: Option<String>,
    pub temperature: Option<f32>,
    pub max_output_tokens: Option<u32>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedRunMetadata {
    pub evaluate_version: &'static str,
    pub dataset: BenchmarkDataset,
    pub input: PathBuf,
    pub output_dir: PathBuf,
    pub backend: ResolvedBackendMetadata,
    pub answerer: ResolvedAnswererMetadata,
    pub retrieval: RetrievalBudget,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedBackendMetadata {
    pub kind: BackendKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedAnswererMetadata {
    pub kind: AnswererKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_compatible: Option<ResolvedOpenAiCompatibleAnswererMetadata>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedOpenAiCompatibleAnswererMetadata {
    pub base_url: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TomlRunConfig {
    run: TomlRunSection,
    backend: TomlBackendSection,
    retrieval: Option<TomlRetrievalSection>,
    answerer: TomlAnswererSection,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TomlRunSection {
    dataset: BenchmarkDataset,
    input: PathBuf,
    output_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TomlBackendSection {
    kind: BackendKind,
    return_all: Option<EmptySection>,
    oracle: Option<EmptySection>,
    kioku: Option<EmptySection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TomlRetrievalSection {
    max_items: Option<usize>,
    max_tokens: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TomlAnswererSection {
    kind: AnswererKind,
    debug: Option<EmptySection>,
    openai_compatible: Option<TomlOpenAiCompatibleSection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TomlOpenAiCompatibleSection {
    base_url: String,
    model: String,
    api_key_env: Option<String>,
    temperature: Option<f32>,
    max_output_tokens: Option<u32>,
    timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptySection {}

pub fn load_run_config(path: impl AsRef<Path>) -> anyhow::Result<RunConfig> {
    Ok(parse_config_file(path)?.into_resolved()?.validate()?.run)
}

pub fn parse_config_file(path: impl AsRef<Path>) -> anyhow::Result<ParsedConfig> {
    let source_path = path.as_ref().to_path_buf();
    let raw_bytes = std::fs::read(&source_path)
        .with_context(|| format!("failed to read config `{}`", source_path.display()))?;
    let raw_text = std::str::from_utf8(&raw_bytes)
        .with_context(|| format!("config `{}` is not valid UTF-8", source_path.display()))?;
    let toml = toml::from_str::<TomlRunConfig>(raw_text)
        .with_context(|| format!("failed to parse TOML config `{}`", source_path.display()))?;

    Ok(ParsedConfig {
        source_path,
        raw_bytes,
        toml,
    })
}

impl ParsedConfig {
    pub fn into_resolved(self) -> anyhow::Result<ResolvedConfig> {
        let config_dir = absolute_path(
            self.source_path
                .parent()
                .unwrap_or_else(|| Path::new(".")),
        )
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
            retrieval: RetrievalBudget {
                max_items: retrieval.max_items,
                max_tokens: retrieval.max_tokens,
            },
        };

        Ok(ResolvedConfig {
            source_path,
            raw_bytes: self.raw_bytes,
            toml: self.toml,
            run,
        })
    }
}

impl ResolvedConfig {
    pub fn validate(self) -> anyhow::Result<ValidatedConfig> {
        validate_backend(
            &self.run.backend,
            &self.run.dataset,
            &self.run.retrieval,
            &self,
        )?;
        validate_answerer(&self.run.answerer, &self)?;
        validate_output_dir(&self.run.output_dir)?;
        Ok(ValidatedConfig {
            source_path: self.source_path,
            raw_bytes: self.raw_bytes,
            run: self.run,
        })
    }
}

impl ValidatedConfig {
    pub fn resolved_metadata(&self) -> anyhow::Result<ResolvedRunMetadata> {
        let answerer = match &self.run.answerer {
            AnswererConfig::Debug => ResolvedAnswererMetadata {
                kind: self.run.answerer.kind(),
                openai_compatible: None,
            },
            AnswererConfig::OpenAiCompatible(openai) => ResolvedAnswererMetadata {
                kind: self.run.answerer.kind(),
                openai_compatible: Some(ResolvedOpenAiCompatibleAnswererMetadata {
                    base_url: openai.base_url.clone(),
                    model: openai.model.clone(),
                    api_key_env: openai.api_key_env.clone(),
                    temperature: openai.temperature,
                    max_output_tokens: openai.max_output_tokens,
                    timeout_secs: openai.timeout_secs,
                }),
            },
        };

        Ok(ResolvedRunMetadata {
            evaluate_version: env!("CARGO_PKG_VERSION"),
            dataset: self.run.dataset,
            input: self.run.input.clone(),
            output_dir: self.run.output_dir.clone(),
            backend: ResolvedBackendMetadata {
                kind: self.run.backend.kind,
            },
            answerer,
            retrieval: self.run.retrieval,
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
                "max_tokens is not supported by return-all backend in Phase 1.5"
            );
        }
        BackendKind::Oracle | BackendKind::Kioku => {
            return Err(anyhow!(
                "backend.kind = \"{}\" is not supported in Phase 1.5",
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
        AnswererConfig::OpenAiCompatible(_) => {
            return Err(anyhow!(
                "answerer.kind = \"openai-compatible\" is not supported in Phase 1.5"
            ));
        }
    }

    Ok(())
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
                OpenAiCompatibleAnswererConfig {
                    base_url: openai.base_url.clone(),
                    model: openai.model.clone(),
                    api_key_env: openai.api_key_env.clone(),
                    temperature: openai.temperature,
                    max_output_tokens: openai.max_output_tokens,
                    timeout_secs: openai.timeout_secs,
                },
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AnswererConfig, AnswererKind, BackendConfig, BackendKind, OpenAiCompatibleAnswererConfig,
        ResolvedAnswererMetadata, ResolvedOpenAiCompatibleAnswererMetadata, parse_config_file,
    };
    use crate::model::{BenchmarkDataset, RetrievalBudget};
    use std::path::PathBuf;

    fn write_temp_config(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kioku-evaluate-config-{name}-{}",
            std::process::id()
        ));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("run.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

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
        let config_dir = std::env::current_dir().unwrap().join(path.parent().unwrap());

        assert_eq!(resolved.run.dataset.as_str(), "locomo");
        assert_eq!(resolved.source_path, std::fs::canonicalize(&path).unwrap());
        assert_eq!(resolved.run.input, config_dir.parent().unwrap().join("data/input.json"));
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
        let config_dir = std::env::current_dir().unwrap().join(path.parent().unwrap());

        assert_eq!(resolved.run.input, config_dir.parent().unwrap().join("data/input.json"));
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
    fn rejects_inactive_sections_during_validate() {
        let path = write_temp_config(
            "inactive",
            r#"
[run]
dataset = "locomo"
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"

[answerer.openai_compatible]
base_url = "http://localhost:11434/v1"
model = "test"
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
dataset = "locomo"
input = "input.json"
output_dir = "{}"

[backend]
kind = "return-all"

[answerer]
kind = "debug"
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
"#,
        );

        let resolved = parse_config_file(&path).unwrap().into_resolved().unwrap();
        assert_eq!(resolved.run.backend.kind, BackendKind::Oracle);
        assert_eq!(
            resolved.run.answerer,
            AnswererConfig::OpenAiCompatible(OpenAiCompatibleAnswererConfig {
                base_url: "http://localhost:11434/v1".to_string(),
                model: "test".to_string(),
                api_key_env: Some("OPENAI_API_KEY".to_string()),
                temperature: None,
                max_output_tokens: None,
                timeout_secs: None,
            })
        );

        let error = resolved.validate().unwrap_err().to_string();
        assert!(error.contains("backend.kind = \"oracle\""));
    }

    #[test]
    fn resolved_metadata_includes_evaluate_version() {
        let path = write_temp_config(
            "resolved-version",
            r#"
[run]
dataset = "locomo"
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "debug"
"#,
        );

        let metadata = parse_config_file(path)
            .unwrap()
            .into_resolved()
            .unwrap()
            .validate()
            .unwrap()
            .resolved_metadata()
            .unwrap();

        assert_eq!(metadata.evaluate_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn resolved_metadata_allows_missing_api_key_env() {
        let path = write_temp_config(
            "resolved-api-key-none",
            r#"
[run]
dataset = "locomo"
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "openai-compatible"

[answerer.openai_compatible]
base_url = "http://localhost:11434/v1"
model = "test"
"#,
        );

        let parsed = parse_config_file(path).unwrap();
        let validated = super::ValidatedConfig {
            source_path: std::fs::canonicalize(&parsed.source_path).unwrap(),
            raw_bytes: parsed.raw_bytes,
            run: super::RunConfig {
                dataset: BenchmarkDataset::LoCoMo,
                input: PathBuf::from("/tmp/input.json"),
                output_dir: PathBuf::from("/tmp/out"),
                backend: BackendConfig {
                    kind: BackendKind::ReturnAll,
                },
                answerer: AnswererConfig::OpenAiCompatible(OpenAiCompatibleAnswererConfig {
                    base_url: "http://localhost:11434/v1".to_string(),
                    model: "test".to_string(),
                    api_key_env: None,
                    temperature: None,
                    max_output_tokens: None,
                    timeout_secs: None,
                }),
                retrieval: RetrievalBudget::default(),
            },
        };
        let metadata = validated.resolved_metadata().unwrap();

        assert_eq!(
            metadata.answerer,
            ResolvedAnswererMetadata {
                kind: AnswererKind::OpenAiCompatible,
                openai_compatible: Some(ResolvedOpenAiCompatibleAnswererMetadata {
                    base_url: "http://localhost:11434/v1".to_string(),
                    model: "test".to_string(),
                    api_key_env: None,
                    temperature: None,
                    max_output_tokens: None,
                    timeout_secs: None,
                }),
            }
        );
    }
}
