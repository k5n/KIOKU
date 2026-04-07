use serde_json::json;

use crate::model::{AnswerRequest, BenchmarkDataset, RetrievedMemory};

#[derive(Debug, Clone, PartialEq)]
pub struct LlmPrompt {
    pub system_prompt: Option<String>,
    pub user_prompt: String,
    pub metadata: serde_json::Value,
}

const SYSTEM_PROMPT: &str = concat!(
    "You answer benchmark questions using only the provided memories.\n",
    "If the memories do not contain enough evidence, say that the information is insufficient."
);

pub fn build_llm_prompt(request: AnswerRequest<'_>) -> LlmPrompt {
    let mut lines = vec![
        format!("Dataset: {}", request.dataset.as_str()),
        dataset_guidance(request.dataset).to_string(),
        format!("Case ID: {}", request.case.case_id),
        format!("Question ID: {}", request.question.question_id),
    ];

    if let Some(question_type) = &request.question.question_type {
        lines.push(format!("Question Type: {question_type}"));
    }

    if let Some(category) = request.question.category {
        lines.push(format!("Category: {category}"));
    }

    lines.push(format!(
        "Requires Abstention: {}",
        request.question.is_abstention
    ));
    lines.push(String::new());
    lines.push("Question:".to_string());
    lines.push(request.question.question.clone());
    lines.push(String::new());
    lines.push("Retrieved Memories:".to_string());

    if request.retrieved.is_empty() {
        lines.push("(none)".to_string());
    } else {
        lines.extend(
            request
                .retrieved
                .iter()
                .enumerate()
                .map(format_memory_for_prompt),
        );
    }

    lines.push(String::new());
    lines.push(
        "Answer with the shortest complete response you can justify from the retrieved memories."
            .to_string(),
    );

    LlmPrompt {
        system_prompt: Some(SYSTEM_PROMPT.to_string()),
        user_prompt: lines.join("\n"),
        metadata: json!({
            "dataset": request.dataset.as_str(),
            "question_id": request.question.question_id,
            "question_type": request.question.question_type,
            "category": request.question.category,
            "is_abstention": request.question.is_abstention,
        }),
    }
}

fn dataset_guidance(dataset: BenchmarkDataset) -> &'static str {
    match dataset {
        BenchmarkDataset::LoCoMo => {
            "Dataset Guidance: LoCoMo questions may require temporal or multi-hop reasoning over dialogue turns."
        }
        BenchmarkDataset::LongMemEval => {
            "Dataset Guidance: LongMemEval questions may require resolving details across multiple sessions and abstaining when evidence is missing."
        }
    }
}

fn format_memory_for_prompt((index, memory): (usize, &RetrievedMemory)) -> String {
    let speaker = memory
        .speaker_name
        .as_deref()
        .or(memory.speaker_id.as_deref())
        .unwrap_or("unknown-speaker");
    format!(
        "{}. [stream={} timestamp={} speaker={}]\n---\n{}\n---",
        index + 1,
        memory.stream_id,
        memory.timestamp,
        speaker,
        memory.content
    )
}

#[cfg(test)]
mod tests {
    use super::build_llm_prompt;
    use crate::model::{
        AnswerRequest, BenchmarkCase, BenchmarkDataset, BenchmarkQuestion, GoldAnswerVariant,
        RetrievedMemory,
    };

    #[test]
    fn prompt_builder_includes_common_and_dataset_specific_context() {
        let retrieved = [RetrievedMemory {
            event_id: "event-1".to_string(),
            stream_id: "session-1".to_string(),
            timestamp: "2024-01-01T10:00:00Z".to_string(),
            content: "The user moved to Kyoto.".to_string(),
            speaker_id: Some("user".to_string()),
            speaker_name: Some("User".to_string()),
            metadata: serde_json::Value::Null,
        }];
        let request = sample_request(BenchmarkDataset::LongMemEval, false, &retrieved);

        let prompt = build_llm_prompt(request);

        assert_eq!(
            prompt.system_prompt.as_deref(),
            Some(
                "You answer benchmark questions using only the provided memories.\nIf the memories do not contain enough evidence, say that the information is insufficient."
            )
        );
        assert!(prompt.user_prompt.contains("Dataset: longmemeval"));
        assert!(prompt.user_prompt.contains("Dataset Guidance: LongMemEval"));
        assert!(prompt.user_prompt.contains("Question Type: multi-session"));
        assert!(prompt.user_prompt.contains("Category: 4"));
        assert!(prompt.user_prompt.contains("speaker=User"));
        assert_eq!(prompt.metadata["dataset"], "longmemeval");
        assert_eq!(prompt.metadata["question_type"], "multi-session");
        assert_eq!(prompt.metadata["category"], 4);
        assert_eq!(prompt.metadata["is_abstention"], false);
    }

    #[test]
    fn prompt_builder_handles_empty_retrievals() {
        let prompt = build_llm_prompt(sample_request(BenchmarkDataset::LoCoMo, true, &[]));

        assert!(prompt.user_prompt.contains("Dataset: locomo"));
        assert!(prompt.user_prompt.contains("Dataset Guidance: LoCoMo"));
        assert!(prompt.user_prompt.contains("Requires Abstention: true"));
        assert!(prompt.user_prompt.contains("Retrieved Memories:\n(none)"));
    }

    fn sample_request<'a>(
        dataset: BenchmarkDataset,
        is_abstention: bool,
        retrieved: &'a [RetrievedMemory],
    ) -> AnswerRequest<'a> {
        let case = Box::leak(Box::new(BenchmarkCase {
            dataset,
            case_id: format!("{}:case-1", dataset.as_str()),
            events: Vec::new(),
            questions: Vec::new(),
            metadata: serde_json::Value::Null,
        }));
        let question = Box::leak(Box::new(BenchmarkQuestion {
            question_id: format!("{}:case-1:q0", dataset.as_str()),
            question: "Where does the user live now?".to_string(),
            question_timestamp: None,
            gold_answers: vec!["Kyoto".to_string()],
            evidence_event_ids: Vec::new(),
            evidence_session_ids: Vec::new(),
            category: Some(4),
            question_type: Some("multi-session".to_string()),
            gold_answer_variant: GoldAnswerVariant::Default,
            is_abstention,
            metadata: serde_json::Value::Null,
        }));

        AnswerRequest {
            dataset,
            case,
            question,
            retrieved,
        }
    }
}
