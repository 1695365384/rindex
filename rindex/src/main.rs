use rindex::config::Config;
use rindex::db::Database;
use rindex::ignore::{IgnoreConfig, IgnoreEngine};
use rindex::mcp::{AppState, McpHandler, format_response, parse_request};
use rindex::watcher;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Mutex};

fn main() -> Result<()> {
    // Parse CLI before initializing tracing (so --help and --version don't log)
    let config = Config::load()
        .context("Failed to load configuration")?;

    // Configure logging: JSON format for production, human-readable for dev
    let log_format = std::env::var("RINDEX_LOG_FORMAT").unwrap_or_default();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("rindex=info".parse()
                    .context("Invalid tracing filter directive")?)
        );
    if log_format == "json" {
        subscriber.json().init();
        tracing::info!("JSON logging enabled via RINDEX_LOG_FORMAT=json");
    } else {
        subscriber.init();
    }

    tracing::info!("rindex v{} starting...", env!("CARGO_PKG_VERSION"));

    let root_path = config.project_root.clone();
    let root_str = root_path.to_string_lossy().to_string();

    tracing::info!("Project root: {:?}", root_path);
    tracing::info!("Database: {:?}", config.db_path);

    // Initialize database
    let db = Database::open(&config.db_path)?;

    // Setup ignore engine
    let ignore_cfg = IgnoreConfig { max_file_size: config.max_file_size };
    let mut ignore = IgnoreEngine::new(&ignore_cfg);

    // Load .gitignore if it exists
    let gitignore_path = root_path.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)
        .with_context(|| format!("Failed to read {:?}", gitignore_path))?;
        for line in content.lines() {
            ignore.add_gitignore_pattern(line);
        }
        tracing::info!("Loaded .gitignore patterns");
    }

    let config_arc = Arc::new(config);

    // Auto-index in background (embeddings added lazily on first search)
    let db_arc = Arc::new(Mutex::new(db));
    let ignore_arc = Arc::new(ignore);

    {
        let db = db_arc.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
        let proj = rindex::db::queries::get_or_create_project(&db, &root_str)?;
        if proj.file_count == 0 {
            tracing::info!("First time indexing project (text-only, embeddings on demand)...");
            let (_handle, rx) = rindex::indexer::index_project(
                Arc::clone(&db_arc),
                None, // No embedder at startup — loaded lazily
                Arc::clone(&ignore_arc),
                &root_path,
                None, // IndexState managed internally
            );

            std::thread::spawn(move || {
                while let Ok(progress) = rx.recv() {
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

    // Create MCP handler (embedder loads lazily on first search)
    let state = AppState {
        root_path: root_str,
        db: db_arc,
        config: config_arc,
        embedder: Mutex::new(None),
        ignore: ignore_arc,
    };

    // Start file watcher for incremental re-indexing
    let change_rx = watcher::FileWatcher::start(&root_path)
        .map_err(|e| tracing::warn!("File watcher failed to start: {}", e))
        .ok();
    if change_rx.is_some() {
        tracing::info!("File watcher active, incremental re-indexing enabled");
    }

    // Create copies for watcher before moving state into handler
    let watch_db = state.db.clone();
    let watch_ignore = state.ignore.clone();
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

        // Drain any file changes before processing the request
        if let Some(ref rx) = change_rx {
            loop {
                match rx.try_recv() {
                    Ok(changes) => {
                        watcher::process_changes(&changes, &watch_db, &watch_ignore, &root_path);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
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
