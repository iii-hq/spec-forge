use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GenerateRequest {
    pub prompt: String,
    pub catalog: Catalog,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub stream: bool,
}

pub fn default_model() -> String {
    "claude-opus-4-6".into()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Catalog {
    pub components: BTreeMap<String, ComponentDef>,
    #[serde(default)]
    pub actions: BTreeMap<String, ActionDef>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ComponentDef {
    pub description: String,
    #[serde(default)]
    pub props: serde_json::Value,
    #[serde(default)]
    pub children: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ActionDef {
    pub description: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UISpec {
    pub root: String,
    pub elements: HashMap<String, UIElement>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UIElement {
    #[serde(rename = "type")]
    pub element_type: String,
    pub props: serde_json::Value,
    #[serde(default)]
    pub children: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GenerateResponse {
    pub spec: UISpec,
    pub cached: bool,
    pub generation_ms: u64,
    pub model: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<String>>,
}
