use serde::Deserialize;
use std::path::PathBuf;

use crate::model::BenchmarkDataset;

use super::{AnswererKind, BackendKind, JudgeKind};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TomlRunConfig {
    pub(super) run: TomlRunSection,
    pub(super) backend: TomlBackendSection,
    pub(super) retrieval: Option<TomlRetrievalSection>,
    pub(super) prompt: Option<TomlPromptSection>,
    pub(super) answerer: TomlAnswererSection,
    pub(super) judge: Option<TomlJudgeSection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TomlRunSection {
    pub(super) dataset: BenchmarkDataset,
    pub(super) input: PathBuf,
    pub(super) output_dir: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TomlBackendSection {
    pub(super) kind: BackendKind,
    pub(super) return_all: Option<EmptySection>,
    pub(super) oracle: Option<EmptySection>,
    pub(super) kioku: Option<EmptySection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TomlRetrievalSection {
    pub(super) max_items: Option<usize>,
    pub(super) max_tokens: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TomlAnswererSection {
    pub(super) kind: AnswererKind,
    pub(super) debug: Option<EmptySection>,
    pub(super) openai_compatible: Option<TomlOpenAiCompatibleSection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TomlPromptSection {
    pub(super) longmemeval_kioku: Option<TomlLongMemEvalKiokuPromptSection>,
    pub(super) locomo_kioku: Option<TomlLocomoKiokuPromptSection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TomlLocomoKiokuPromptSection {
    pub(super) answer_template_id: String,
    pub(super) answer_judge_prompt_id: String,
    pub(super) retrieval_judge_prompt_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TomlLongMemEvalKiokuPromptSection {
    pub(super) answer_template_id: String,
    pub(super) answer_judge_prompt_id: String,
    pub(super) retrieval_judge_prompt_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TomlOpenAiCompatibleSection {
    pub(super) base_url: String,
    pub(super) model: String,
    pub(super) api_key_env: String,
    pub(super) temperature: f32,
    pub(super) max_output_tokens: u32,
    pub(super) timeout_secs: u64,
    pub(super) max_retries: u32,
    pub(super) retry_backoff_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TomlJudgeSection {
    pub(super) kind: JudgeKind,
    pub(super) openai_compatible: Option<TomlOpenAiCompatibleSection>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EmptySection {}
