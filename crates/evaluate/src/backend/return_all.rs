use anyhow::ensure;
use async_trait::async_trait;

use crate::backend::MemoryBackend;
use crate::model::{BenchmarkEvent, EvalScope, QueryInput, QueryOutput};
use crate::prompt::{PromptContext, PromptContextKind};

#[derive(Debug, Default)]
pub struct ReturnAllMemoryBackend {
    scope: Option<EvalScope>,
    events: Vec<BenchmarkEvent>,
}

#[async_trait]
impl MemoryBackend for ReturnAllMemoryBackend {
    async fn reset(&mut self, scope: EvalScope) -> anyhow::Result<()> {
        self.scope = Some(scope);
        self.events.clear();
        Ok(())
    }

    async fn ingest(&mut self, event: BenchmarkEvent) -> anyhow::Result<()> {
        // This backend is a stub for end-to-end validation, so it preserves the
        // ingestion order instead of reordering events by timestamp.
        self.events.push(event);
        Ok(())
    }

    async fn query(&mut self, input: QueryInput) -> anyhow::Result<QueryOutput> {
        ensure!(
            input.budget.max_tokens.is_none(),
            "max_tokens is not supported by return-all backend in Phase 1",
        );

        if let Some(scope) = &self.scope {
            ensure!(
                scope == &input.scope,
                "query scope mismatch: expected case `{}`, got `{}`",
                scope.case_id,
                input.scope.case_id,
            );
        }

        let mut selected_events: Vec<_> = self.events.iter().collect();
        if let Some(max_items) = input.budget.max_items {
            let start = selected_events.len().saturating_sub(max_items);
            selected_events = selected_events.split_off(start);
        }

        let prompt_context = build_prompt_context(&selected_events);

        Ok(QueryOutput {
            prompt_context,
            metadata: serde_json::json!({
                "backend_kind": "return_all",
            }),
        })
    }
}

fn build_prompt_context(events: &[&BenchmarkEvent]) -> PromptContext {
    PromptContext {
        kind: PromptContextKind::MemoryPrompt,
        text: render_memory_prompt(events),
        metadata: serde_json::json!({
            "backend_kind": "return_all",
            "renderer": "event-memory-prompt-v1",
        }),
    }
}

fn render_memory_prompt(events: &[&BenchmarkEvent]) -> String {
    if events.is_empty() {
        return "(none)".to_string();
    }

    events
        .iter()
        .enumerate()
        .map(|(index, event)| {
            let speaker = event
                .speaker_name
                .as_deref()
                .or(event.speaker_id.as_deref())
                .unwrap_or("unknown");
            let mut lines = vec![
                format!("## Memory {}", index + 1),
                format!("Memory ID: {}", event.event_id),
                format!("Stream ID: {}", event.stream_id),
                format!("Timestamp: {}", event.timestamp),
                format!("Speaker: {speaker}"),
            ];
            if !event.metadata.is_null() {
                lines.push(format!("Metadata: {}", event.metadata));
            }
            lines.push("Content:".to_string());
            lines.push(event.content.trim().to_string());
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::ReturnAllMemoryBackend;
    use crate::backend::MemoryBackend;
    use crate::model::{BenchmarkDataset, BenchmarkEvent, EvalScope, QueryInput, RetrievalBudget};
    use crate::prompt::PromptContextKind;

    fn event(id: &str, timestamp: &str) -> BenchmarkEvent {
        BenchmarkEvent {
            event_id: id.to_string(),
            stream_id: "stream".to_string(),
            timestamp: timestamp.to_string(),
            content: id.to_string(),
            speaker_id: None,
            speaker_name: None,
            metadata: serde_json::Value::Null,
        }
    }

    fn longmemeval_event(
        id: &str,
        stream_id: &str,
        timestamp: &str,
        session_date: &str,
        content: &str,
    ) -> BenchmarkEvent {
        BenchmarkEvent {
            event_id: id.to_string(),
            stream_id: stream_id.to_string(),
            timestamp: timestamp.to_string(),
            content: content.to_string(),
            speaker_id: Some("user".to_string()),
            speaker_name: Some("user".to_string()),
            metadata: serde_json::json!({
                "session_date": session_date,
            }),
        }
    }

    #[tokio::test]
    async fn query_returns_last_n_items_in_prompt_context_order() {
        let mut backend = ReturnAllMemoryBackend::default();
        let scope = EvalScope {
            dataset: BenchmarkDataset::LoCoMo,
            case_id: "locomo:sample".to_string(),
        };
        backend.reset(scope.clone()).await.unwrap();
        backend
            .ingest(event("e1", "2024-01-01T00:00:00Z"))
            .await
            .unwrap();
        backend
            .ingest(event("e3", "2024-01-01T00:00:02Z"))
            .await
            .unwrap();
        backend
            .ingest(event("e2", "2024-01-01T00:00:01Z"))
            .await
            .unwrap();

        let output = backend
            .query(QueryInput {
                scope,
                question_id: "q1".to_string(),
                query: "what".to_string(),
                timestamp: None,
                budget: RetrievalBudget {
                    max_items: Some(2),
                    max_tokens: None,
                },
                metadata: serde_json::Value::Null,
            })
            .await
            .unwrap();

        let context = output.prompt_context;
        assert_eq!(context.kind, PromptContextKind::MemoryPrompt);
        assert!(context.text.contains("Memory ID: e3"));
        assert!(context.text.contains("Memory ID: e2"));
        assert!(!context.text.contains("Memory ID: e1"));
    }

    #[tokio::test]
    async fn query_returns_memory_prompt_context_for_all_datasets() {
        let mut backend = ReturnAllMemoryBackend::default();
        let scope = EvalScope {
            dataset: BenchmarkDataset::LongMemEval,
            case_id: "longmemeval:sample".to_string(),
        };
        backend.reset(scope.clone()).await.unwrap();
        backend
            .ingest(event("e1", "2024-01-01T00:00:00Z"))
            .await
            .unwrap();

        let output = backend
            .query(QueryInput {
                scope,
                question_id: "q1".to_string(),
                query: "what".to_string(),
                timestamp: None,
                budget: RetrievalBudget::default(),
                metadata: serde_json::Value::Null,
            })
            .await
            .unwrap();

        assert_eq!(output.prompt_context.kind, PromptContextKind::MemoryPrompt);
    }

    #[tokio::test]
    async fn memory_prompt_preserves_ingest_order() {
        let mut backend = ReturnAllMemoryBackend::default();
        let scope = EvalScope {
            dataset: BenchmarkDataset::LongMemEval,
            case_id: "longmemeval:sample".to_string(),
        };
        backend.reset(scope.clone()).await.unwrap();
        backend
            .ingest(longmemeval_event(
                "e1",
                "s1",
                "2024-01-01T00:00:10Z",
                "2024-01-01",
                "first session",
            ))
            .await
            .unwrap();
        backend
            .ingest(longmemeval_event(
                "e2",
                "s2",
                "2024-01-01T00:00:00Z",
                "2024-01-02",
                "second session",
            ))
            .await
            .unwrap();

        let output = backend
            .query(QueryInput {
                scope,
                question_id: "q1".to_string(),
                query: "what".to_string(),
                timestamp: None,
                budget: RetrievalBudget::default(),
                metadata: serde_json::Value::Null,
            })
            .await
            .unwrap();

        let context = output.prompt_context;
        assert_eq!(context.kind, PromptContextKind::MemoryPrompt);
        assert!(context.text.contains("Memory ID: e1"));
        assert!(context.text.contains("Memory ID: e2"));
        assert!(
            context.text.find("first session").unwrap()
                < context.text.find("second session").unwrap()
        );
    }

    #[tokio::test]
    async fn query_returns_deterministic_memory_prompt_context() {
        let mut backend = ReturnAllMemoryBackend::default();
        let scope = EvalScope {
            dataset: BenchmarkDataset::LoCoMo,
            case_id: "locomo:sample".to_string(),
        };
        backend.reset(scope.clone()).await.unwrap();
        backend
            .ingest(event("e1", "2024-01-01T00:00:00Z"))
            .await
            .unwrap();
        backend
            .ingest(event("e2", "2024-01-01T00:00:01Z"))
            .await
            .unwrap();

        let output = backend
            .query(QueryInput {
                scope,
                question_id: "q1".to_string(),
                query: "what".to_string(),
                timestamp: None,
                budget: RetrievalBudget::default(),
                metadata: serde_json::Value::Null,
            })
            .await
            .unwrap();

        let context = output.prompt_context;
        assert_eq!(context.kind, PromptContextKind::MemoryPrompt);
        assert!(context.text.contains("Memory ID: e1"));
        assert!(context.text.contains("Memory ID: e2"));
    }
}
