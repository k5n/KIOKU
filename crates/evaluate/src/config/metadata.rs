use super::{
    AnswererConfig, ResolvedAnswererMetadata, ResolvedBackendMetadata,
    ResolvedOpenAiCompatibleAnswererMetadata, ResolvedPromptMetadata, ResolvedRunMetadata,
    ValidatedConfig,
};

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
            prompt: ResolvedPromptMetadata {
                longmemeval: self.run.prompt.longmemeval,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        AnswererKind, ResolvedAnswererMetadata, ResolvedOpenAiCompatibleAnswererMetadata,
        parse_config_file,
    };
    use crate::config::test_support::write_temp_config;

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
    fn resolved_metadata_includes_openai_retry_settings() {
        let path = write_temp_config(
            "resolved-openai-metadata",
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
api_key_env = "OPENAI_API_KEY"
temperature = 0.0
max_output_tokens = 256
timeout_secs = 45
max_retries = 3
retry_backoff_ms = 250
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
        assert_eq!(metadata.prompt.longmemeval, None);
    }
}
