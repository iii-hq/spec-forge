use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct GenerateRequest {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub catalog: Catalog,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub stream: bool,
}

pub fn default_model() -> String {
    "claude-sonnet-4-6".into()
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct Catalog {
    #[serde(default)]
    pub components: BTreeMap<String, ComponentDef>,
    #[serde(default)]
    pub actions: BTreeMap<String, ActionDef>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ComponentDef {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub props: serde_json::Value,
    #[serde(default)]
    pub children: bool,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct ActionDef {
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct UISpec {
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub elements: HashMap<String, UIElement>,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct UIElement {
    #[serde(rename = "type", default)]
    pub element_type: String,
    #[serde(default)]
    pub props: serde_json::Value,
    #[serde(default)]
    pub children: Vec<String>,
}
