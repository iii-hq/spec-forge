use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
pub struct GenerateRequest {
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub catalog: Catalog,
    #[serde(default)]
    pub catalog_preset: Option<String>,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub strict: bool,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub origin_peer: Option<String>,
}

pub fn default_max_tokens() -> u32 {
    4096
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct JoinSessionRequest {
    pub session_id: String,
    #[serde(default)]
    pub worker_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LeaveSessionRequest {
    pub session_id: String,
    pub worker_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PushPatchRequest {
    pub session_id: String,
    #[serde(default)]
    pub origin_peer: Option<String>,
    #[serde(default)]
    pub patch: serde_json::Value,
}

impl JoinSessionRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.session_id.is_empty() {
            return Err("session_id is required".to_string());
        }
        Ok(())
    }
}

impl LeaveSessionRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.session_id.is_empty() {
            return Err("session_id is required".to_string());
        }
        if self.worker_id.is_empty() {
            return Err("worker_id is required".to_string());
        }
        Ok(())
    }
}

impl PushPatchRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.session_id.is_empty() {
            return Err("session_id is required".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub peers: Vec<String>,
    pub spec: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub spec: serde_json::Value,
    pub timestamp: u64,
    pub author: String,
}
