use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreparedPrompt {
    pub system_prompt: Option<String>,
    pub user_prompt: String,
    pub template_id: String,
    #[serde(default)]
    pub metadata: Value,
}

impl PreparedPrompt {
    pub fn prompt_metadata(&self) -> Value {
        let mut metadata = match self.metadata.clone() {
            Value::Object(map) => map,
            Value::Null => Map::new(),
            other => {
                let mut map = Map::new();
                map.insert("details".to_string(), other);
                map
            }
        };
        metadata.insert(
            "template_id".to_string(),
            Value::String(self.template_id.clone()),
        );
        Value::Object(metadata)
    }
}
