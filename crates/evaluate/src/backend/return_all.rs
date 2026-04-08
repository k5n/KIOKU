use anyhow::{bail, ensure};
use async_trait::async_trait;

use crate::backend::MemoryBackend;
use crate::model::{BenchmarkEvent, EvalScope, QueryInput, QueryOutput, RetrievedMemory};
use crate::prompt::{LongMemEvalAnswerPromptProfile, PromptContext, PromptContextKind};

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
        if matches!(
            input.requested_longmemeval_prompt_profile,
            Some(
                LongMemEvalAnswerPromptProfile::HistoryChatsWithFacts
                    | LongMemEvalAnswerPromptProfile::FactsOnly
            )
        ) {
            bail!(
                "return-all backend cannot satisfy LongMemEval prompt profile `{}`",
                input.requested_longmemeval_prompt_profile.unwrap().as_str()
            );
        }

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

        let retrieved = if matches!(
            input.requested_longmemeval_prompt_profile,
            Some(LongMemEvalAnswerPromptProfile::NoRetrieval)
        ) {
            Vec::new()
        } else {
            selected_events
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
                .collect()
        };
        let prompt_context = build_prompt_context(&input, &self.events);

        Ok(QueryOutput {
            retrieved,
            prompt_context,
            metadata: serde_json::json!({
                "backend_kind": "return_all",
            }),
        })
    }
}

fn build_prompt_context(input: &QueryInput, events: &[BenchmarkEvent]) -> Option<PromptContext> {
    match input.scope.dataset {
        crate::model::BenchmarkDataset::LoCoMo => None,
        crate::model::BenchmarkDataset::LongMemEval => {
            if matches!(
                input.requested_longmemeval_prompt_profile,
                Some(LongMemEvalAnswerPromptProfile::NoRetrieval)
            ) {
                return Some(PromptContext {
                    kind: PromptContextKind::NoRetrieval,
                    text: String::new(),
                    metadata: serde_json::Value::Null,
                });
            }

            Some(PromptContext {
                kind: PromptContextKind::HistoryChats,
                text: render_history_chats(events, input.budget.max_items),
                metadata: serde_json::json!({
                    "backend_kind": "return_all",
                }),
            })
        }
    }
}

fn render_history_chats(events: &[BenchmarkEvent], max_items: Option<usize>) -> String {
    let selected_events = if let Some(max_items) = max_items {
        let start = events.len().saturating_sub(max_items);
        &events[start..]
    } else {
        events
    };

    let mut sessions: Vec<(String, String, Vec<String>)> = Vec::new();
    for event in selected_events {
        let session_date = event
            .metadata
            .get("session_date")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&event.timestamp);
        let speaker = event
            .speaker_name
            .as_deref()
            .or(event.speaker_id.as_deref())
            .unwrap_or("unknown");
        let line = format!("{speaker}: {}", event.content.trim());

        if let Some((stream_id, _, lines)) = sessions.last_mut()
            && stream_id == &event.stream_id
        {
            lines.push(line);
            continue;
        }

        sessions.push((
            event.stream_id.clone(),
            session_date.to_string(),
            vec![line],
        ));
    }

    sessions
        .into_iter()
        .enumerate()
        .map(|(index, (_, session_date, lines))| {
            format!(
                "### Session {}:\nSession Date: {}\nSession Content:\n{}",
                index + 1,
                session_date,
                lines.join("\n\n")
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::ReturnAllMemoryBackend;
    use crate::backend::MemoryBackend;
    use crate::model::{BenchmarkDataset, BenchmarkEvent, EvalScope, QueryInput, RetrievalBudget};
    use crate::prompt::{LongMemEvalAnswerPromptProfile, PromptContextKind};

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
    async fn query_returns_last_n_items_in_ingest_order() {
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
                requested_longmemeval_prompt_profile: None,
                metadata: serde_json::Value::Null,
            })
            .await
            .unwrap();

        let event_ids: Vec<_> = output
            .retrieved
            .into_iter()
            .map(|item| item.event_id)
            .collect();
        assert_eq!(event_ids, vec!["e3".to_string(), "e2".to_string()]);
    }

    #[tokio::test]
    async fn query_fails_fast_for_unsupported_facts_profiles() {
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

        let error = backend
            .query(QueryInput {
                scope,
                question_id: "q1".to_string(),
                query: "what".to_string(),
                timestamp: None,
                budget: RetrievalBudget::default(),
                requested_longmemeval_prompt_profile: Some(
                    LongMemEvalAnswerPromptProfile::FactsOnly,
                ),
                metadata: serde_json::Value::Null,
            })
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("cannot satisfy"));
        assert!(error.contains("facts-only"));
    }

    #[tokio::test]
    async fn query_returns_no_retrieval_context_with_empty_retrieved_items() {
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
                requested_longmemeval_prompt_profile: Some(
                    LongMemEvalAnswerPromptProfile::NoRetrieval,
                ),
                metadata: serde_json::Value::Null,
            })
            .await
            .unwrap();

        assert!(output.retrieved.is_empty());
        assert_eq!(
            output.prompt_context.as_ref().map(|context| &context.kind),
            Some(&PromptContextKind::NoRetrieval)
        );
    }

    #[tokio::test]
    async fn history_chat_prompt_context_preserves_ingest_order() {
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
                requested_longmemeval_prompt_profile: Some(
                    LongMemEvalAnswerPromptProfile::HistoryChats,
                ),
                metadata: serde_json::Value::Null,
            })
            .await
            .unwrap();

        let context = output.prompt_context.expect("prompt context");
        assert_eq!(context.kind, PromptContextKind::HistoryChats);
        assert!(
            context
                .text
                .contains("### Session 1:\nSession Date: 2024-01-01")
        );
        assert!(
            context
                .text
                .contains("### Session 2:\nSession Date: 2024-01-02")
        );
        assert!(
            context.text.find("first session").unwrap()
                < context.text.find("second session").unwrap()
        );
    }
}
