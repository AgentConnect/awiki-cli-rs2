//! Model metadata shared by durable state and the ACP transport.
use serde_json::{json, Value};

pub fn set_current_model(options: &mut Value, model: &str) {
    if let Some(items) = options.as_array_mut() {
        if let Some(item) = items
            .iter_mut()
            .find(|v| v["category"] == "model" || v["id"] == "model")
        {
            item["currentValue"] = json!(model);
        }
    } else if options.is_object() {
        options["currentModelId"] = json!(model);
    }
}

pub fn current_model(options: &Value) -> Option<String> {
    let current = if let Some(items) = options.as_array() {
        items
            .iter()
            .find(|item| item["category"] == "model" || item["id"] == "model")?
            .get("currentValue")?
            .as_str()
    } else {
        options["currentModelId"].as_str()
    };
    current
        .filter(|id| !id.trim().is_empty())
        .map(str::to_owned)
}

pub fn session_options(session: &Value) -> Value {
    session
        .get("configOptions")
        .filter(|value| {
            value.as_array().is_some_and(|v| {
                v.iter()
                    .any(|item| item["category"] == "model" || item["id"] == "model")
                    || (!v.is_empty() && session.get("models").is_none())
            })
        })
        .or_else(|| session.get("models"))
        .cloned()
        .unwrap_or(json!([]))
}

pub fn model_options(options: &Value) -> Vec<Value> {
    fn choices(value: &Value, result: &mut Vec<Value>) {
        if let Some(items) = value.as_array() {
            for item in items {
                if let Some(id) = item["value"].as_str().or(item["modelId"].as_str()) {
                    let mut model = json!({"id":id,"name":item["name"].as_str().unwrap_or(id)});
                    if let Some(description) = item["description"].as_str() {
                        model["description"] = json!(description);
                    }
                    result.push(model);
                } else {
                    choices(&item["options"], result);
                }
            }
        }
    }
    let mut result = vec![];
    if let Some(items) = options.as_array() {
        for item in items
            .iter()
            .filter(|v| v["category"] == "model" || v["id"] == "model")
        {
            choices(&item["options"], &mut result);
        }
    } else {
        choices(&options["availableModels"], &mut result);
    }
    result
}
