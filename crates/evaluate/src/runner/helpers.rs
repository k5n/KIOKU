use serde_json::Value;

use crate::prompt::PromptContext;

pub(super) fn extract_answerer_model(metadata: &Value) -> String {
    metadata
        .get("llm")
        .and_then(|llm| llm.get("model_name"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            metadata
                .get("answerer")
                .and_then(|answerer| answerer.get("kind"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| "unknown".to_string())
}

pub(super) fn sanitize_answer_metadata(
    mut metadata: Value,
    template_id: &str,
    answerer_model: &str,
) -> Value {
    if let Some(llm) = metadata.get_mut("llm").and_then(Value::as_object_mut) {
        llm.remove("raw_response");
    }

    if let Some(prompt) = metadata.get_mut("prompt").and_then(Value::as_object_mut) {
        prompt.insert(
            "template_id".to_string(),
            Value::String(template_id.to_string()),
        );
    }

    if let Some(root) = metadata.as_object_mut() {
        root.insert(
            "template_id".to_string(),
            Value::String(template_id.to_string()),
        );
        root.insert(
            "answerer_model".to_string(),
            Value::String(answerer_model.to_string()),
        );
    }

    metadata
}

pub(super) fn context_kind_name(context: &PromptContext) -> String {
    serde_json::to_value(&context.kind)
        .ok()
        .and_then(|value| value.as_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}
