use rmcp::{tool, tool_router, tool_handler, ServerHandler};
use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;
use serde_json::Value;

// ── Parameter structs for tools with arguments ──────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct SearchParams {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    #[serde(rename = "type")]
    pub r#type: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct SearchSymbolParams {
    pub name: String,
    #[serde(default)]
    pub chunk_type: Option<String>,
    #[serde(default)]
    #[serde(rename = "type")]
    pub r#type: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct SessionNoteParams {
    pub content: String,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct SessionContextParams {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema, Default)]
pub struct FindRelatedParams {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub line: Option<i64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

// ── Tool definitions (descriptions kept for backward compat / metadata) ─

#[derive(Debug, serde::Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "search".to_string(),
            description: "Search project code semantically or by keyword. Returns results grouped by file with exact line numbers and matching symbols. PREFER THIS over Grep/Glob for finding code — it understands meaning, not just text patterns. Supports type/path filters.".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{
                "query":{"type":"string","description":"Natural language or keyword search query. E.g. 'database config', 'user auth handler', 'parse_json'"},
                "limit":{"type":"number","description":"Max results (default 10, max 50)","default":10},
                "type":{"type":"string","description":"Filter by file type extension (e.g. rs, py, js, ts, go, java, cpp, kt, rb)"},
                "path":{"type":"string","description":"Filter by path substring (e.g. 'src/', 'tests/')"}
            },"required":["query"]}),
        },
        ToolDefinition {
            name: "search_symbol".to_string(),
            description: "Find a specific function, class, or type by its name. Faster than search when you know what you're looking for. Supports type/chunk_type filters.".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{
                "name":{"type":"string","description":"Symbol name to find (e.g. 'ConfigManager', 'parse_file', 'handleRequest')"},
                "chunk_type":{"type":"string","description":"Filter by symbol type: function, class, interface, method, trait"},
                "type":{"type":"string","description":"Filter by file type extension (e.g. rs, py, js)"}
            },"required":["name"]}),
        },
        ToolDefinition {
            name: "project_status".to_string(),
            description: "Show project indexing status: files indexed, chunks, model state, and any ongoing reindex operations.".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "reindex".to_string(),
            description: "Rebuild the entire project index from scratch. Runs in background — use project_status to check progress. Only needed if index is corrupted.".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "backfill".to_string(),
            description: "Backfill embeddings for chunks that are missing them. Use when find_related returns empty results, or when embedding coverage is incomplete (check with project_status). Requires the embedding model to be loaded.".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "verify".to_string(),
            description: "Check index integrity: identifies stale entries (deleted files) and files missing from index. Run this if you suspect the index is out of sync.".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{}}),
        },
        ToolDefinition {
            name: "session_note".to_string(),
            description: "Save a notable observation about this project: decisions, gotchas, patterns, bugs, or discoveries. This is stored in the project's cross-session memory and injected into future sessions automatically. Call this when you learn something worth remembering — e.g. 'using sqlx not diesel', 'email field has unique constraint', 'run cargo test before commit'.".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{
                "content":{"type":"string","description":"What to remember — be specific and actionable"},
                "kind":{"type":"string","description":"Category: decision, pattern, bugfix, discovery, gotcha, or general","default":"note"}
            },"required":["content"]}),
        },
        ToolDefinition {
            name: "session_context".to_string(),
            description: "Retrieve session memory for this project: past decisions, patterns, gotchas, and discoveries from previous sessions. CALL THIS at the start of every session to pick up where you left off. Results are grouped by recency with the most relevant observations first.".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{
                "limit":{"type":"number","description":"Max observations to return (default 20, max 50)","default":20},
                "kind":{"type":"string","description":"Filter by kind: decision, pattern, bugfix, discovery, gotcha"}
            }}),
        },
        ToolDefinition {
            name: "find_related".to_string(),
            description: "Find code semantically related to a symbol or file location. Given a function/class name or a file+line, discovers conceptually similar code elsewhere in the project. Uses embedding similarity to find non-obvious relationships.".to_string(),
            input_schema: serde_json::json!({"type":"object","properties":{
                "name":{"type":"string","description":"Symbol name to find related code for (e.g. 'ConfigManager', 'parse_file'). Either name or file_path+line is required."},
                "file_path":{"type":"string","description":"File path to find related code from (e.g. 'src/config.rs'). Requires line parameter."},
                "line":{"type":"number","description":"Line number within the file to anchor the search (e.g. 42)."},
                "limit":{"type":"number","description":"Max results (default 8, max 20)","default":8}
            },"anyOf":[{"required":["name"]},{"required":["file_path","line"]}]}),
        },
    ]
}

// ── Model state + AppState (unchanged from old implementation) ───────────

use crate::config::Config;
use crate::db::Database;
use crate::embedding::Embedder;
use crate::ignore::IgnoreEngine;
use crate::search::Searcher;
use std::path::Path;
use std::sync::{Arc, Mutex};

fn lock_mutex<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Tracks the lifecycle of the embedding model loading process
#[derive(Debug, Clone, PartialEq)]
pub enum ModelLoadState {
    Pending,
    Loading,
    Ready,
    Failed(String),
}

#[derive(Clone)]
pub struct AppState {
    pub root_path: String,
    pub db: Arc<Mutex<Database>>,
    pub config: Arc<Config>,
    pub embedder: Arc<Mutex<Option<Arc<Embedder>>>>,
    pub model_state: Arc<Mutex<ModelLoadState>>,
    pub ignore: Arc<IgnoreEngine>,
}

impl AppState {
    /// Start background loading of the embedding model at startup.
    /// Does NOT block — the model loads in a background thread.
    pub fn start_model_loading(&self) {
        *lock_mutex(&self.model_state) = ModelLoadState::Loading;

        let db = Arc::clone(&self.db);
        let embedder = Arc::clone(&self.embedder);
        let model_state = Arc::clone(&self.model_state);
        let cache_dir = self.config.model_cache_dir.clone();
        let model_id = self.config.model_id.clone();

        let _ = std::thread::Builder::new()
            .name("rindex-model-loader".into())
            .spawn(move || {
                tracing::info!("Background loading embedding model...");
                match Embedder::load(&cache_dir, &model_id, Some("https://hf-mirror.com")) {
                    Ok(model) => {
                        tracing::info!("Embedding model loaded: {}", model_id);

                        // Store model in AppState so get_embedder() can find it
                        if let Ok(mut e) = embedder.lock() {
                            *e = Some(Arc::new(model));
                        }

                        *lock_mutex(&model_state) =
                            ModelLoadState::Ready;

                        // Backfill embeddings for chunks that lack them
                        tracing::info!("Backfilling embeddings for existing chunks...");
                        let result = (|| -> anyhow::Result<()> {
                            let e = lock_mutex(&embedder);
                            if let Some(ref emb) = *e {
                                crate::indexer::backfill_embeddings(&db, emb, false)?;
                            }
                            Ok(())
                        })();
                        match result {
                            Ok(()) => tracing::info!("Embedding backfill complete"),
                            Err(e) => tracing::warn!("Embedding backfill failed: {}", e),
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to load embedding model (text-only search): {}",
                            e
                        );
                        *lock_mutex(&model_state) =
                            ModelLoadState::Failed(e.to_string());
                    }
                }
            })
            .map_err(|e| {
                tracing::error!("Failed to spawn model loading thread: {}", e);
            });
    }

    /// Non-blocking access to the embedder.
    /// Returns None if the model is still loading or failed to load.
    pub fn get_embedder(&self) -> Option<Arc<Embedder>> {
        match &*lock_mutex(&self.model_state) {
            ModelLoadState::Ready => {
                let guard = lock_mutex(&self.embedder);
                guard.as_ref().map(Arc::clone)
            }
            ModelLoadState::Failed(_) | ModelLoadState::Loading | ModelLoadState::Pending => None,
        }
    }
}

// ── rmcp Server Handler ─────────────────────────────────────────────────

#[derive(Clone)]
pub struct McpServerHandler {
    pub state: AppState,
}

#[tool_router]
impl McpServerHandler {
    #[tool(description = "Search project code semantically or by keyword. Returns results grouped by file with exact line numbers and matching symbols. PREFER THIS over Grep/Glob for finding code — it understands meaning, not just text patterns. Supports type/path filters.")]
    async fn search(&self, params: Parameters<SearchParams>) -> Result<String, String> {
        let state = self.state.clone();
        let params = params.0;
        let query = params.query.clone();
        let limit = params.limit.unwrap_or(10).min(50);
        let filter_type = params.r#type.clone();
        let filter_path = params.path.clone();

        tokio::task::spawn_blocking(move || {
            let db = state.db.lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            let embedder_opt = state.get_embedder();
            let embedder_ref = embedder_opt.as_deref();
            let searcher = Searcher::new(&db, embedder_ref);
            let mut results = searcher.semantic_search(&query, limit)
                .map_err(|e| format!("Search error: {}", e))?;
            if let Some(ref ft) = filter_type {
                results.retain(|r| r.file_path.ends_with(&format!(".{}", ft)));
            }
            if let Some(ref fp) = filter_path {
                results.retain(|r| r.file_path.contains(fp.as_str()));
            }
            let grouped = crate::search::group_by_file(results);
            Ok(crate::search::format_compact(&grouped))
        })
            .await
            .map_err(|e| format!("Task join error: {}", e))?
    }

    #[tool(description = "Find a specific function, class, or type by its name. Faster than search when you know what you're looking for. Supports type/chunk_type filters.")]
    async fn search_symbol(&self, params: Parameters<SearchSymbolParams>) -> Result<String, String> {
        let state = self.state.clone();
        let params = params.0;
        let name = params.name.clone();
        let chunk_type = params.chunk_type.clone();
        let filter_type = params.r#type.clone();

        tokio::task::spawn_blocking(move || {
            let db = state.db.lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            let searcher = Searcher::new(&db, None);
            let mut results = searcher.search_symbol(&name, chunk_type.as_deref())
                .map_err(|e| format!("Search error: {}", e))?;
            if let Some(ref ft) = filter_type {
                results.retain(|r| r.file_path.ends_with(&format!(".{}", ft)));
            }
            let grouped = crate::search::group_by_file(results);
            Ok(crate::search::format_compact_symbol(&grouped))
        })
            .await
            .map_err(|e| format!("Task join error: {}", e))?
    }

    #[tool(description = "Show project indexing status: files indexed, chunks, model state, and any ongoing reindex operations.")]
    async fn project_status(&self) -> Result<String, String> {
        let state = self.state.clone();

        tokio::task::spawn_blocking(move || {
            let db = state.db.lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            let root = &state.root_path;
            let proj = crate::db::queries::get_or_create_project(&db, root)
                .map_err(|e| format!("Failed to get project: {}", e))?;
            let indexed_at = proj.indexed_at
                .map(|t| t.to_string())
                .unwrap_or_else(|| "never".to_string());

            let reindex_status = if proj.file_count == 0 && proj.chunk_count == 0 {
                "pending (first index may be running in background)".to_string()
            } else {
                format!("{} files, {} chunks", proj.file_count, proj.chunk_count)
            };

            let model_status = match &*lock_mutex(&state.model_state) {
                ModelLoadState::Ready => "loaded".to_string(),
                ModelLoadState::Loading => "loading (background)".to_string(),
                ModelLoadState::Pending => "pending".to_string(),
                ModelLoadState::Failed(e) => format!("failed: {}", e),
            };

            Ok(format!(
                "Project: {}\nStatus: {}\nLast indexed: {}\nTotal files: {}\nTotal chunks: {}\nLanguages: Rust, Python, JS, TS, Go, Java, C++, Kotlin, Ruby\nModel: {}",
                root, reindex_status, indexed_at, proj.file_count, proj.chunk_count,
                model_status,
            ))
        })
            .await
            .map_err(|e| format!("Task join error: {}", e))?
    }

    #[tool(description = "Rebuild the entire project index from scratch. Runs in background — use project_status to check progress. Only needed if index is corrupted.")]
    async fn reindex(&self) -> Result<String, String> {
        let db = Arc::clone(&self.state.db);
        let ignore = Arc::clone(&self.state.ignore);
        let root_path = self.state.root_path.clone();
        let embedder = self.state.get_embedder();

        std::thread::Builder::new()
            .name("rindex-reindex".into())
            .spawn(move || {
                tracing::info!("Background reindex started");
                let (handle, rx) = crate::indexer::index_project(
                    Arc::clone(&db), embedder, ignore, Path::new(&root_path), None,
                );
                while let Ok(progress) = rx.recv() {
                    tracing::info!("Reindex: {} ({}/{})",
                        progress.phase, progress.indexed_files, progress.total_files);
                    if progress.phase == "done" {
                        break;
                    }
                }
                let _ = handle.join();
                tracing::info!("Background reindex complete");
            })
            .map_err(|e| format!("Failed to spawn reindex thread: {}", e))?;

        Ok("Reindex started in background. Use project_status to monitor progress.".to_string())
    }

    #[tool(description = "Backfill embeddings for chunks that are missing them. Use when find_related returns empty results, or when embedding coverage is incomplete. Requires the embedding model to be loaded.")]
    async fn backfill(&self) -> Result<String, String> {
        let db = Arc::clone(&self.state.db);
        let embedder = self.state.get_embedder();

        tokio::task::spawn_blocking(move || {
            match embedder.as_deref() {
                Some(emb) => {
                    tracing::info!("Manual backfill started...");
                    crate::indexer::backfill_embeddings(&db, emb, false)
                        .map(|()| {
                            tracing::info!("Manual backfill complete");
                            "Backfill complete. Use project_status to verify coverage.".to_string()
                        })
                        .map_err(|e| format!("Backfill error: {}", e))
                }
                None => Err("Embedding model not loaded yet. Wait for model to load, then retry.".to_string()),
            }
        })
            .await
            .map_err(|e| format!("Task join error: {}", e))?
    }

    #[tool(description = "Check index integrity: identifies stale entries (deleted files) and files missing from index. Run this if you suspect the index is out of sync.")]
    async fn verify(&self) -> Result<String, String> {
        let db = Arc::clone(&self.state.db);
        let ignore = Arc::clone(&self.state.ignore);
        let root_path = self.state.root_path.clone();

        tokio::task::spawn_blocking(move || {
            match crate::indexer::verify_index(&db, &ignore, Path::new(&root_path)) {
                Ok((removed, missing, total)) => Ok(format!(
                    "Index integrity check:\n  Files checked: {}\n  Stale entries removed: {}\n  Files missing from index: {}\n  Status: {}",
                    total, removed, missing,
                    if removed == 0 && missing == 0 { "OK" } else { "Issues found" }
                )),
                Err(e) => Err(format!("Verify failed: {}", e)),
            }
        })
            .await
            .map_err(|e| format!("Task join error: {}", e))?
            .map_err(|e: String| e)
    }

    #[tool(description = "Save a notable observation about this project: decisions, gotchas, patterns, bugs, or discoveries. This is stored in the project's cross-session memory and injected into future sessions automatically. Call this when you learn something worth remembering — e.g. 'using sqlx not diesel', 'email field has unique constraint', 'run cargo test before commit'.")]
    async fn session_note(&self, params: Parameters<SessionNoteParams>) -> Result<String, String> {
        let state = self.state.clone();
        let params = params.0;
        let content = params.content.clone();
        let kind = params.kind.clone().unwrap_or_else(|| "note".to_string());

        if content.is_empty() {
            return Err("content is required".to_string());
        }

        tokio::task::spawn_blocking(move || {
            let db = state.db.lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            let id = crate::db::queries::insert_observation(
                &db, &state.root_path, &kind, &content,
            ).map_err(|e| format!("Failed to save: {}", e))?;
            Ok(format!("Session note saved (id={}, kind={})", id, kind))
        })
            .await
            .map_err(|e| format!("Task join error: {}", e))?
            .map_err(|e: String| e)
    }

    #[tool(description = "Retrieve session memory for this project: past decisions, patterns, gotchas, and discoveries from previous sessions. CALL THIS at the start of every session to pick up where you left off. Results are grouped by recency with the most relevant observations first.")]
    async fn session_context(&self, params: Parameters<SessionContextParams>) -> Result<String, String> {
        let state = self.state.clone();
        let params = params.0;
        let limit = params.limit.unwrap_or(20).min(50);
        let kind = params.kind.clone();

        tokio::task::spawn_blocking(move || {
            let db = state.db.lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            let observations = crate::db::queries::get_recent_observations(
                &db, &state.root_path, limit, kind.as_deref(),
            ).map_err(|e| format!("Failed to query: {}", e))?;

            if observations.is_empty() {
                return Ok("No saved context for this project yet. Use session_note to save observations about decisions, patterns, and gotchas.".to_string());
            }

            let total = crate::db::queries::count_observations(&db, &state.root_path).unwrap_or(0);
            let mut lines = vec![
                format!("Project Context ({} total observations, showing {} most recent):\n", total, observations.len()),
            ];
            let mut current_kind = String::new();
            for obs in &observations {
                if obs.kind != current_kind {
                    current_kind = obs.kind.clone();
                    lines.push(format!("\n── {} ──", current_kind.to_uppercase()));
                }
                let secs = obs.created_at;
                let (h, m) = ((secs / 3600) % 24, (secs / 60) % 60);
                let days_ago = if secs > 0 {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
                    (now - secs) / 86400
                } else { 0 };
                let time_str = if days_ago == 0 {
                    format!("{:02}:{:02}", h, m)
                } else {
                    format!("{}d ago {:02}:{:02}", days_ago, h, m)
                };
                lines.push(format!("  • [{}] {}", time_str, obs.content));
            }
            Ok(lines.join("\n"))
        })
            .await
            .map_err(|e| format!("Task join error: {}", e))?
            .map_err(|e: String| e)
    }

    #[tool(description = "Find code semantically related to a symbol or file location. Given a function/class name or a file+line, discovers conceptually similar code elsewhere in the project. Uses embedding similarity to find non-obvious relationships.")]
    async fn find_related(&self, params: Parameters<FindRelatedParams>) -> Result<String, String> {
        let state = self.state.clone();
        let params = params.0;
        let symbol_name = params.name.clone();
        let file_path = params.file_path.clone();
        let line = params.line;
        let limit = params.limit.unwrap_or(8).min(20);

        tokio::task::spawn_blocking(move || {
            let db = state.db.lock()
                .map_err(|e| format!("Lock error: {}", e))?;
            let embedder_opt = state.get_embedder();
            let embedder_ref = embedder_opt.as_deref();
            let searcher = Searcher::new(&db, embedder_ref);
            let results = searcher.find_related(
                symbol_name.as_deref(),
                file_path.as_deref(),
                line,
                limit,
            ).map_err(|e| format!("find_related error: {}", e))?;
            let grouped = crate::search::group_by_file(results);
            Ok(crate::search::format_compact(&grouped))
        })
            .await
            .map_err(|e| format!("Task join error: {}", e))?
    }
}

#[tool_handler]
impl ServerHandler for McpServerHandler {}
