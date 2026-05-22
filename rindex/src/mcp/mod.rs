use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

#[derive(Debug, Serialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "search".to_string(),
            description: "Semantically search project code, returning results with file paths and line numbers. More efficient than Glob/Grep.".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{"query":{"type":"string","description":"Search query"},"limit":{"type":"number","description":"Max results (default 10)","default":10}},"required":["query"]}),
        },
        ToolDefinition {
            name: "search_symbol".to_string(),
            description: "Search for a symbol by exact name (function, class, etc.)".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{"name":{"type":"string","description":"Symbol name"},"chunk_type":{"type":"string","description":"Filter by type"}},"required":["name"]}),
        },
        ToolDefinition {
            name: "project_status".to_string(),
            description: "Get project index status (indexed files, total chunks)".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "reindex".to_string(),
            description: "Trigger a full reindex of the project".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{}}),
        },
    ]
}

pub fn parse_request(line: &str) -> Result<McpRequest> {
    Ok(serde_json::from_str(line)?)
}

pub fn format_response(resp: &McpResponse) -> String {
    serde_json::to_string(resp).unwrap() + "\n"
}
