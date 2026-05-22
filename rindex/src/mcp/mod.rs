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

use crate::db::Database;
use crate::embedding::Embedder;
use crate::ignore::IgnoreEngine;
use crate::search::Searcher;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

pub struct AppState {
    pub root_path: String,
    pub db: Arc<Mutex<Database>>,
    pub embedder: Option<Arc<Embedder>>,
    pub ignore: Arc<IgnoreEngine>,
}

pub struct McpHandler {
    pub state: AppState,
}

impl McpHandler {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub fn handle_request(&self, req: McpRequest) -> McpResponse {
        let id = req.id.unwrap_or(serde_json::Value::Null);

        let result = match req.method.as_str() {
            "initialize" => Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "rindex", "version": "0.1.0" }
            })),
            "notifications/initialized" => Some(serde_json::Value::Null),
            "tools/list" => Some(serde_json::json!({
                "tools": get_tool_definitions()
            })),
            "tools/call" => {
                let tool_name = req.params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = req.params.get("arguments").unwrap_or(&serde_json::Value::Null);
                match self.handle_tool_call(tool_name, args) {
                    Ok(content) => Some(serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": content
                        }]
                    })),
                    Err(e) => {
                        return McpResponse {
                            jsonrpc: "2.0".to_string(),
                            id,
                            result: None,
                            error: Some(McpError {
                                code: -32603,
                                message: format!("Tool error: {}", e),
                                data: None,
                            }),
                        };
                    }
                }
            }
            _ => {
                return McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id,
                    result: None,
                    error: Some(McpError {
                        code: -32601,
                        message: format!("Method not found: {}", req.method),
                        data: None,
                    }),
                };
            }
        };

        McpResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result,
            error: None,
        }
    }

    fn handle_tool_call(&self, name: &str, args: &serde_json::Value) -> Result<String, String> {
        match name {
            "search" => {
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

                let db = self.state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
                let searcher = Searcher::new(&db, self.state.embedder.as_deref());
                let results = searcher.semantic_search(query, limit)
                    .map_err(|e| format!("Search error: {}", e))?;
                serde_json::to_string_pretty(&results)
                    .map_err(|e| format!("Serialize error: {}", e))
            }
            "search_symbol" => {
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let chunk_type = args.get("chunk_type").and_then(|v| v.as_str());

                let db = self.state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
                let searcher = Searcher::new(&db, self.state.embedder.as_deref());
                let results = searcher.search_symbol(name, chunk_type)
                    .map_err(|e| format!("Search error: {}", e))?;
                serde_json::to_string_pretty(&results)
                    .map_err(|e| format!("Serialize error: {}", e))
            }
            "project_status" => {
                let db = self.state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
                let root = &self.state.root_path;
                let proj = crate::db::queries::get_or_create_project(&db, root)
                    .unwrap_or_default();
                Ok(format!(
                    "Project: {}\nIndexed files: {}\nTotal chunks: {}\nLast indexed: {}",
                    root, proj.file_count, proj.chunk_count,
                    proj.indexed_at.map(|t| t.to_string()).unwrap_or("never".to_string())
                ))
            }
            "reindex" => {
                let db = Arc::clone(&self.state.db);
                let embedder = self.state.embedder.clone();
                let ignore = Arc::clone(&self.state.ignore);
                let root_path = self.state.root_path.clone();

                // Start indexing in background thread
                let (handle, rx) = crate::indexer::index_project(db, embedder, ignore, Path::new(&root_path));

                // Wait for completion
                while let Ok(progress) = rx.recv() {
                    if progress.phase == "done" {
                        break;
                    }
                }

                handle.join().map_err(|_| "Index thread panicked".to_string())?
                    .map_err(|e| format!("Index error: {}", e))?;

                Ok("Reindex complete".to_string())
            }
            _ => Err(format!("Unknown tool: {}", name)),
        }
    }
}
