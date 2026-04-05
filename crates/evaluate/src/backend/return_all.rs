use anyhow::ensure;
use async_trait::async_trait;

use crate::backend::MemoryBackend;
use crate::model::{BenchmarkEvent, EvalScope, QueryInput, QueryOutput, RetrievedMemory};

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
        self.events.push(event);
        self.events
            .sort_by(|left, right| left.timestamp.cmp(&right.timestamp));
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

        let retrieved = selected_events
            .into_iter()
            .map(|event| RetrievedMemory {
                event_id: event.event_id.clone(),
                stream_id: event.stream_id.clone(),
                timestamp: event.timestamp.clone(),
                content: event.content.clone(),
                speaker_id: event.speaker_id.clone(),
                speaker_name: event.speaker_name.clone(),
                metadata: event.metadata.clone(),
            })
            .collect();

        Ok(QueryOutput {
            retrieved,
            metadata: serde_json::json!({
                "backend_kind": "return_all",
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ReturnAllMemoryBackend;
    use crate::backend::MemoryBackend;
    use crate::model::{BenchmarkDataset, BenchmarkEvent, EvalScope, QueryInput, RetrievalBudget};

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

    #[tokio::test]
    async fn query_returns_newest_n_items_in_chronological_order() {
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

        let event_ids: Vec<_> = output
            .retrieved
            .into_iter()
            .map(|item| item.event_id)
            .collect();
        assert_eq!(event_ids, vec!["e2".to_string(), "e3".to_string()]);
    }
}
