use rindex::config::Config;
use rindex::db::Database;
use rindex::embedding::Embedder;
use rindex::ignore::{IgnoreConfig, IgnoreEngine};
use rindex::mcp::{AppState, McpHandler, format_response, parse_request};
use anyhow::Result;
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rindex=info".parse()?)
        )
        .init();

    tracing::info!("rindex starting...");

    let root = std::env::current_dir()?;
    let config = Config::from_project_root(&root);

    // Initialize database
    let db = Database::open(&config.db_path)?;
    tracing::info!("Database initialized at {:?}", config.db_path);

    // Setup ignore engine
    let ignore_cfg = IgnoreConfig { max_file_size: config.max_file_size };
    let mut ignore = IgnoreEngine::new(&ignore_cfg);

    // Load .gitignore if it exists
    let gitignore_path = root.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        for line in content.lines() {
            ignore.add_gitignore_pattern(line);
        }
        tracing::info!("Loaded .gitignore patterns");
    }

    // Load embedding model (optional, non-fatal if fails)
    let embedder = match Embedder::load(&config.model_cache_dir, &config.model_id) {
        Ok(model) => {
            tracing::info!("Embedding model loaded: {}", config.model_id);
            Some(Arc::new(model))
        }
        Err(e) => {
            tracing::warn!("Failed to load embedding model (text-only search): {}", e);
            None
        }
    };

    // Auto-index if needed
    let root_str = root.to_string_lossy().to_string();
    let db_arc = Arc::new(Mutex::new(db));
    let ignore_arc = Arc::new(ignore);

    {
        let db = db_arc.lock().unwrap();
        let proj = rindex::db::queries::get_or_create_project(&db, &root_str)?;
        if proj.file_count == 0 {
            tracing::info!("First time indexing project...");
            let (_tx, _rx) = std::sync::mpsc::channel::<rindex::indexer::IndexProgress>();
            let (_handle, _rx) = rindex::indexer::index_project(
                Arc::clone(&db_arc),
                embedder.clone(),
                Arc::clone(&ignore_arc),
                &root,
            );

            std::thread::spawn(move || {
                while let Ok(progress) = _rx.recv() {
                    match progress.phase.as_str() {
                        "scanning" => tracing::info!("Scanning project files..."),
                        "indexing" => tracing::info!("Indexing: {}/{} files, {} chunks",
                            progress.indexed_files, progress.total_files, progress.total_chunks),
                        "done" => tracing::info!("Index complete: {} files, {} chunks",
                            progress.total_files, progress.total_chunks),
                        _ => {}
                    }
                }
            });
        } else {
            tracing::info!("Project already indexed ({} files, {} chunks)", proj.file_count, proj.chunk_count);
        }
    }

    // Create MCP handler
    let state = AppState {
        root_path: root_str,
        db: db_arc,
        embedder,
        ignore: ignore_arc,
    };
    let handler = McpHandler::new(state);

    // MCP event loop
    tracing::info!("rindex ready, entering MCP event loop");
    let stdin = std::io::stdin();
    let reader = BufReader::new(stdin.lock());
    let mut stdout = std::io::stdout().lock();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        match parse_request(&line) {
            Ok(req) => {
                let resp = handler.handle_request(req);
                let output = format_response(&resp);
                stdout.write_all(output.as_bytes())?;
                stdout.flush()?;
            }
            Err(e) => {
                tracing::error!("Failed to parse request: {}", e);
                let error_resp = rindex::mcp::McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: serde_json::Value::Null,
                    result: None,
                    error: Some(rindex::mcp::McpError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                        data: None,
                    }),
                };
                stdout.write_all(format_response(&error_resp).as_bytes())?;
                stdout.flush()?;
            }
        }
    }

    Ok(())
}
