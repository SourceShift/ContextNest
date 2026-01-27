use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    pub path: String,
    pub last_modified: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Screen {
    pub name: String,
    pub route_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Widget {
    pub id: String,
    pub widget_type: String,
    pub source_code: String,
    pub start_offset: usize,
    pub end_offset: usize,
    pub properties: serde_json::Value,
    pub vector_embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Style {
    pub name: String,
    pub value: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub is_dark_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessKPI {
    pub name: String,
    pub description: String,
    pub target_value: String,
    pub current_value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueLog {
    pub id: String,
    pub description: String,
    pub error_log: String,
    pub resolution: String,
    pub status: IssueStatus,
    pub vector_embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueStatus {
    Open,
    Resolved,
    InProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Relationship {
    Declares,
    Contains,
    IsChildOf,
    NavigatesTo,
    UsesStyle,
    AppliesTheme,
    Impacts,
    HadIssue,
}

impl Widget {
    pub fn new(
        widget_type: String,
        source_code: String,
        start_offset: usize,
        end_offset: usize,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            widget_type,
            source_code,
            start_offset,
            end_offset,
            properties: serde_json::Value::Object(serde_json::Map::new()),
            vector_embedding: None,
        }
    }
}

impl IssueLog {
    pub fn new(description: String, error_log: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            description,
            error_log,
            resolution: String::new(),
            status: IssueStatus::Open,
            vector_embedding: None,
        }
    }
}
