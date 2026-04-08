use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::model::{BenchmarkDataset, RetrievalBudget};
use crate::prompt::LongMemEvalPromptConfig;

use super::toml::TomlRunConfig;

#[derive(Debug, Clone)]
pub struct ParsedConfig {
    pub source_path: PathBuf,
    pub raw_bytes: Vec<u8>,
    pub(super) toml: TomlRunConfig,
}

#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub source_path: PathBuf,
    pub raw_bytes: Vec<u8>,
    pub(super) toml: TomlRunConfig,
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
    pub prompt: PromptConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptConfig {
    pub longmemeval: Option<LongMemEvalPromptConfig>,
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
    pub api_key_env: String,
    pub temperature: f32,
    pub max_output_tokens: u32,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
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
    pub prompt: ResolvedPromptMetadata,
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
    pub api_key_env: String,
    pub temperature: f32,
    pub max_output_tokens: u32,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedPromptMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longmemeval: Option<LongMemEvalPromptConfig>,
}
