use super::{
    AnswererConfig, JudgeConfig, ResolvedAnswererMetadata, ResolvedBackendMetadata,
    ResolvedJudgeMetadata, ResolvedOpenAiCompatibleAnswererMetadata,
    ResolvedOpenAiCompatibleJudgeMetadata, ResolvedPromptMetadata, ResolvedRunMetadata,
    ValidatedConfig,
};
use crate::token_counter::WhitespaceTokenCounter;

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
                    max_retries: openai.max_retries,
                    retry_backoff_ms: openai.retry_backoff_ms,
                }),
            },
        };
        let judge = match &self.run.judge {
            None => None,
            Some(JudgeConfig::OpenAiCompatible(openai)) => Some(ResolvedJudgeMetadata {
                kind: self.run.judge.as_ref().map_or_else(
                    || unreachable!("judge existence already matched"),
                    JudgeConfig::kind,
                ),
                openai_compatible: Some(ResolvedOpenAiCompatibleJudgeMetadata {
                    base_url: openai.base_url.clone(),
                    model: openai.model.clone(),
                    api_key_env: openai.api_key_env.clone(),
                    temperature: openai.temperature,
                    max_output_tokens: openai.max_output_tokens,
                    timeout_secs: openai.timeout_secs,
                    max_retries: openai.max_retries,
                    retry_backoff_ms: openai.retry_backoff_ms,
                }),
            }),
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
            judge,
            retrieval: self.run.retrieval,
            prompt: ResolvedPromptMetadata {
                longmemeval_kioku: self.run.prompt.longmemeval_kioku.clone(),
                locomo_kioku: self.run.prompt.locomo_kioku.clone(),
            },
            context_tokenizer: self
                .run
                .prompt
                .longmemeval_kioku
                .as_ref()
                .map(|_| WhitespaceTokenCounter::NAME.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        AnswererKind, JudgeKind, ResolvedAnswererMetadata, ResolvedJudgeMetadata,
        ResolvedOpenAiCompatibleAnswererMetadata, ResolvedOpenAiCompatibleJudgeMetadata,
        parse_config_file,
    };
    use crate::config::test_support::write_temp_config;
    use crate::prompt::{LocomoKiokuPromptConfig, LongMemEvalKiokuPromptConfig};

    #[test]
    fn resolved_metadata_includes_evaluate_version() {
        let path = write_temp_config(
            "resolved-version",
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

[prompt.longmemeval_kioku]
answer_template_id = "longmemeval.kioku.answer.v1"
answer_judge_prompt_id = "longmemeval.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "longmemeval.kioku.judge.retrieval.v1"
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
    fn resolved_metadata_includes_openai_retry_settings() {
        let path = write_temp_config(
            "resolved-openai-metadata",
            r#"
[run]
dataset = "longmemeval"
input = "input.json"
output_dir = "out"

[backend]
kind = "return-all"

[answerer]
kind = "openai-compatible"

[answerer.openai_compatible]
base_url = "http://localhost:11434/v1"
model = "test"
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 256
timeout_secs = 45
max_retries = 3
retry_backoff_ms = 250

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
        );

        let validated = parse_config_file(path)
            .unwrap()
            .into_resolved()
            .unwrap()
            .validate()
            .unwrap();
        let metadata = validated.resolved_metadata().unwrap();

        assert_eq!(
            metadata.answerer,
            ResolvedAnswererMetadata {
                kind: AnswererKind::OpenAiCompatible,
                openai_compatible: Some(ResolvedOpenAiCompatibleAnswererMetadata {
                    base_url: "http://localhost:11434/v1".to_string(),
                    model: "test".to_string(),
                    api_key_env: "OPENAI_API_KEY".to_string(),
                    temperature: 0.0,
                    max_output_tokens: 256,
                    timeout_secs: 45,
                    max_retries: 3,
                    retry_backoff_ms: 250,
                }),
            }
        );
        assert_eq!(
            metadata.prompt.longmemeval_kioku,
            Some(LongMemEvalKiokuPromptConfig {
                answer_template_id: "longmemeval.kioku.answer.v1".to_string(),
                answer_judge_prompt_id: "longmemeval.kioku.judge.answer.v1".to_string(),
                retrieval_judge_prompt_id: "longmemeval.kioku.judge.retrieval.v1".to_string(),
            })
        );
        assert_eq!(metadata.context_tokenizer.as_deref(), Some("whitespace_v1"));
    }

    #[test]
    fn resolved_metadata_preserves_locomo_judge_and_prompt_ids() {
        let path = write_temp_config(
            "resolved-locomo-metadata",
            r#"
[run]
dataset = "locomo"
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

[prompt.locomo_kioku]
answer_template_id = "locomo.kioku.answer.v1"
answer_judge_prompt_id = "locomo.kioku.judge.answer.v1"
retrieval_judge_prompt_id = "locomo.kioku.judge.retrieval.v1"
"#,
        );

        let validated = parse_config_file(path)
            .unwrap()
            .into_resolved()
            .unwrap()
            .validate()
            .unwrap();
        let metadata = validated.resolved_metadata().unwrap();

        assert_eq!(
            metadata.judge,
            Some(ResolvedJudgeMetadata {
                kind: JudgeKind::OpenAiCompatible,
                openai_compatible: Some(ResolvedOpenAiCompatibleJudgeMetadata {
                    base_url: "http://localhost:11434/v1".to_string(),
                    model: "judge-model".to_string(),
                    api_key_env: "OPENAI_API_KEY".to_string(),
                    temperature: 0.0,
                    max_output_tokens: 512,
                    timeout_secs: 60,
                    max_retries: 3,
                    retry_backoff_ms: 500,
                }),
            })
        );
        assert_eq!(
            metadata.prompt.locomo_kioku,
            Some(LocomoKiokuPromptConfig {
                answer_template_id: "locomo.kioku.answer.v1".to_string(),
                answer_judge_prompt_id: "locomo.kioku.judge.answer.v1".to_string(),
                retrieval_judge_prompt_id: "locomo.kioku.judge.retrieval.v1".to_string(),
            })
        );
    }
}
