use rindex::config::Config;
use rindex::db::Database;
use rindex::ignore::{IgnoreConfig, IgnoreEngine};
use rindex::mcp::{AppState, McpServerHandler};
use rindex::watcher;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use dirs;
use rmcp::service::ServiceExt;
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Mutex};

#[derive(Debug, Parser)]
#[command(name = "rindex", version, about = "Local file index MCP server and CLI")]
struct Cli {
    /// Output JSON for CLI subcommands where applicable
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Show project indexing status
    Status,
    /// Semantic or keyword-oriented search
    Search {
        query: String,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long = "type")]
        file_type: Option<String>,
        #[arg(long)]
        path: Option<String>,
    },
    /// Search symbol by name
    Symbol {
        name: String,
        #[arg(long)]
        chunk_type: Option<String>,
        #[arg(long = "type")]
        file_type: Option<String>,
    },
    /// Find semantically related code
    Related {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        file_path: Option<String>,
        #[arg(long)]
        line: Option<i64>,
        #[arg(long, default_value_t = 8)]
        limit: usize,
    },
    /// Save project memory note
    Note {
        content: String,
        #[arg(long)]
        kind: Option<String>,
    },
    /// Show recent project memory notes
    Context {
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        kind: Option<String>,
    },
    /// Verify index integrity
    Verify,
    /// Rebuild index synchronously
    Reindex,
    /// Backfill embeddings for chunks missing them
    Backfill,
    /// Multi-agent setup: register MCP for Claude Code, opencode, and/or Cursor
    Setup {
        #[arg(long, default_value_t = false)]
        claude: bool,
        #[arg(long, default_value_t = false)]
        opencode: bool,
        #[arg(long, default_value_t = false)]
        cursor: bool,
    },
}

/// Load the embedding model for CLI commands (search, related, etc.).
/// Returns None if the model isn't available (text-only fallback).
fn load_embedder_for_cli(config: &Config) -> Option<rindex::embedding::Embedder> {
    match rindex::embedding::Embedder::load(
        &config.model_cache_dir,
        &config.model_id,
        Some("https://hf-mirror.com"),
    ) {
        Ok(e) => {
            eprintln!("Embedding model loaded");
            Some(e)
        }
        Err(e) => {
            eprintln!("Note: embedding model not loaded ({}) — text-only search", e);
            None
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI before initializing tracing (so --help and --version don't log)
    let cli = Cli::parse();
    let config = Config::load()
        .context("Failed to load configuration")?;

    if let Some(cmd) = cli.command {
        return run_cli(config, cmd, cli.json);
    }

    // Configure logging: JSON format for production, human-readable for dev
    let log_format = std::env::var("RINDEX_LOG_FORMAT").unwrap_or_default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
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

    // Load .llm-index-ignore if it exists (takes priority over .gitignore)
    let llm_ignore_path = root_path.join(".llm-index-ignore");
    if llm_ignore_path.exists() {
        let content = std::fs::read_to_string(&llm_ignore_path)
        .with_context(|| format!("Failed to read {:?}", llm_ignore_path))?;
        for line in content.lines() {
            ignore.add_gitignore_pattern(line);
        }
        tracing::info!("Loaded .llm-index-ignore patterns (override)");
    }

    let config_arc = Arc::new(config);

    // ── Auto-setup: detect coding agent & configure ──
    {
        // Claude Code skill (always install, clients ignore unused files)
        let skill_dir = root_path.join(".claude").join("skills").join("rindex");
        let skill_path = skill_dir.join("SKILL.md");
        if !skill_path.exists() {
            std::fs::create_dir_all(&skill_dir).ok();
            std::fs::write(&skill_path, include_str!("../../plugin/skills/rindex/SKILL.md")).ok();
            tracing::info!("skill → .claude/skills/rindex/");
        }

        // .llm-index-ignore
        let ignore_path = root_path.join(".llm-index-ignore");
        if !ignore_path.exists() {
            std::fs::write(&ignore_path, "# rindex exclude patterns (like .gitignore)\n").ok();
        }
    }

    let db_arc = Arc::new(Mutex::new(db));
    let ignore_arc = Arc::new(ignore);

    // Create AppState early so MCP event loop can start immediately.
    // Embedder starts empty, index runs in background — search/project_status
    // work even while indexing is in progress.
    let state = AppState {
        root_path: root_str.clone(),
        db: db_arc.clone(),
        config: config_arc.clone(),
        embedder: Arc::new(Mutex::new(None)),
        model_state: Arc::new(Mutex::new(rindex::mcp::ModelLoadState::Pending)),
        ignore: ignore_arc.clone(),
    };

    // Spawn background indexing so MCP event loop is not blocked.
    // First run: full index_project. Subsequent runs: incremental sync + FTS rebuild.
    {
        let db = Arc::clone(&db_arc);
        let ignore = Arc::clone(&ignore_arc);
        let root = root_path.clone();
        let root_s = root_str.clone();
        std::thread::spawn(move || {
            let proj = match db.lock() {
                Ok(d) => match rindex::db::queries::get_or_create_project(&d, &root_s) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!("Background index: get_or_create_project failed: {}", e);
                        return;
                    }
                },
                Err(e) => {
                    tracing::error!("Background index: DB lock poisoned: {}", e);
                    return;
                }
            };

            if proj.file_count == 0 {
                tracing::info!("First time indexing project (text-only, embeddings on demand)...");
                let (_handle, rx) = rindex::indexer::index_project(
                    Arc::clone(&db),
                    None,
                    Arc::clone(&ignore),
                    &root,
                    None,
                );

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
            } else {
                tracing::info!("Project already indexed ({} files, {} chunks). Verifying...",
                    proj.file_count, proj.chunk_count);
                match rindex::indexer::sync_project_index(&db, None, &ignore, &root) {
                    Ok((synced, removed, total)) => {
                        tracing::info!(
                            "Sync complete: {} synced, {} removed, {} total",
                            synced, removed, total
                        );
                    }
                    Err(e) => {
                        tracing::error!("Background sync failed: {}", e);
                        return;
                    }
                }

                match db.lock() {
                    Ok(d) => {
                        match rindex::db::queries::rebuild_fts_index(&d) {
                            Ok(count) if count > 0 => {
                                tracing::info!("FTS5 index rebuilt: {} chunks indexed", count);
                            }
                            Ok(_) => {}
                            Err(e) => tracing::error!("FTS rebuild failed: {}", e),
                        }
                    }
                    Err(e) => tracing::error!("FTS rebuild: DB lock poisoned: {}", e),
                }
            }

            tracing::info!("Background indexing complete");
        });
    }

    // Start background model loading (does not block startup)
    state.start_model_loading();

    // Start file watcher for incremental re-indexing
    let change_rx = watcher::FileWatcher::start(&root_path)
        .map_err(|e| tracing::warn!("File watcher failed to start: {}", e))
        .ok();
    if change_rx.is_some() {
        tracing::info!("File watcher active, incremental re-indexing enabled");
    }

    let handler = McpServerHandler { state };

    // Spawn file watcher in a tokio task (replaces old stdin loop polling)
    if let Some(rx) = change_rx {
        let watch_db = handler.state.db.clone();
        let watch_ignore = Arc::new(Mutex::new((*handler.state.ignore).clone()));
        let root = root_path.clone();
        tokio::spawn(async move {
            loop {
                match rx.try_recv() {
                    Ok(changes) => {
                        if watcher::process_changes(&changes, &watch_db, &mut *watch_ignore.lock().unwrap(), &root) {
                            tracing::info!("Ignore rules reloaded from .gitignore change");
                        }
                    }
                    Err(TryRecvError::Empty) => {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    Err(TryRecvError::Disconnected) => {
                        tracing::info!("File watcher disconnected");
                        break;
                    }
                }
            }
        });
    }

    // Start MCP server via rmcp — handles initialize, tools/list, and tools/call automatically
    tracing::info!("rindex ready, starting MCP server via rmcp");
    let running = handler.serve(rmcp::transport::io::stdio()).await?;
    running.waiting().await?;

    Ok(())
}

fn build_ignore_engine(root_path: &std::path::Path, max_file_size: u64) -> Result<IgnoreEngine> {
    let ignore_cfg = IgnoreConfig { max_file_size };
    let mut ignore = IgnoreEngine::new(&ignore_cfg);

    let gitignore_path = root_path.join(".gitignore");
    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)
            .with_context(|| format!("Failed to read {:?}", gitignore_path))?;
        for line in content.lines() {
            ignore.add_gitignore_pattern(line);
        }
    }

    let llm_ignore_path = root_path.join(".llm-index-ignore");
    if llm_ignore_path.exists() {
        let content = std::fs::read_to_string(&llm_ignore_path)
            .with_context(|| format!("Failed to read {:?}", llm_ignore_path))?;
        for line in content.lines() {
            ignore.add_gitignore_pattern(line);
        }
    }

    let llm_ignore_path = root_path.join(".llm-index-ignore");
    if llm_ignore_path.exists() {
        let content = std::fs::read_to_string(&llm_ignore_path)
            .with_context(|| format!("Failed to read {:?}", llm_ignore_path))?;
        for line in content.lines() {
            ignore.add_gitignore_pattern(line);
        }
    }

    Ok(ignore)
}

fn print_output<T: serde::Serialize>(value: &T, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{}", serde_json::to_string(&value)?);
    }
    Ok(())
}

fn run_cli(config: Config, cmd: Commands, as_json: bool) -> Result<()> {
    let root_str = config.project_root.to_string_lossy().to_string();

    match cmd {
        Commands::Status => {
            let db = Database::open(&config.db_path)?;
            let proj = rindex::db::queries::get_or_create_project(&db, &root_str)
                .context("Failed to get project record")?;
            let indexed_at = proj.indexed_at
                .map(|t| t.to_string())
                .unwrap_or_else(|| "never".to_string());
            let status = serde_json::json!({
                "project": root_str,
                "last_indexed": indexed_at,
                "files": proj.file_count,
                "chunks": proj.chunk_count,
                "model": "cli mode (loaded on MCP startup)",
            });
            print_output(&status, as_json)?;
        }
        Commands::Search { query, limit, file_type, path } => {
            let db = Database::open(&config.db_path)?;
            let embedder = load_embedder_for_cli(&config);
            let searcher = rindex::search::Searcher::new(&db, embedder.as_ref());
            let mut results = searcher.semantic_search(&query, limit.min(50))?;
            if let Some(ft) = file_type {
                results.retain(|r| r.file_path.ends_with(&format!(".{}", ft)));
            }
            if let Some(fp) = path {
                results.retain(|r| r.file_path.contains(fp.as_str()));
            }
            let grouped = rindex::search::group_by_file(results);
            print_output(&grouped, as_json)?;
        }
        Commands::Symbol { name, chunk_type, file_type } => {
            let db = Database::open(&config.db_path)?;
            let searcher = rindex::search::Searcher::new(&db, None);
            let mut results = searcher.search_symbol(&name, chunk_type.as_deref())?;
            if let Some(ft) = file_type {
                results.retain(|r| r.file_path.ends_with(&format!(".{}", ft)));
            }
            let grouped = rindex::search::group_by_file(results);
            print_output(&grouped, as_json)?;
        }
        Commands::Related { name, file_path, line, limit } => {
            if name.is_none() && (file_path.is_none() || line.is_none()) {
                anyhow::bail!("Provide --name or both --file-path and --line");
            }
            let db = Database::open(&config.db_path)?;
            let embedder = load_embedder_for_cli(&config);
            let searcher = rindex::search::Searcher::new(&db, embedder.as_ref());
            let results = searcher.find_related(
                name.as_deref(),
                file_path.as_deref(),
                line,
                limit.min(20),
            )?;
            let grouped = rindex::search::group_by_file(results);
            print_output(&grouped, as_json)?;
        }
        Commands::Note { content, kind } => {
            let db = Database::open(&config.db_path)?;
            let kind = kind.unwrap_or_else(|| "note".to_string());
            let id = rindex::db::queries::insert_observation(&db, &root_str, &kind, &content)?;
            let out = serde_json::json!({"id": id, "kind": kind, "saved": true});
            print_output(&out, as_json)?;
        }
        Commands::Context { limit, kind } => {
            let db = Database::open(&config.db_path)?;
            let observations = rindex::db::queries::get_recent_observations(
                &db,
                &root_str,
                limit.min(50),
                kind.as_deref(),
            )?;
            print_output(&observations, as_json)?;
        }
        Commands::Verify => {
            let ignore = build_ignore_engine(&config.project_root, config.max_file_size)?;
            let db = Arc::new(Mutex::new(Database::open(&config.db_path)?));
            let (removed, missing, total) = rindex::indexer::verify_index(
                &db,
                &ignore,
                &config.project_root,
            )?;
            let out = serde_json::json!({
                "checked": total,
                "stale_removed": removed,
                "missing": missing,
                "ok": removed == 0 && missing == 0,
            });
            print_output(&out, as_json)?;
        }
        Commands::Reindex => {
            let ignore = Arc::new(build_ignore_engine(&config.project_root, config.max_file_size)?);
            let db = Arc::new(Mutex::new(Database::open(&config.db_path)?));
            let (handle, rx) = rindex::indexer::index_project(
                Arc::clone(&db),
                None,
                Arc::clone(&ignore),
                &config.project_root,
                None,
            );
            let mut last = serde_json::json!({"phase": "starting"});
            while let Ok(progress) = rx.recv() {
                last = serde_json::json!({
                    "phase": progress.phase,
                    "indexed": progress.indexed_files,
                    "total": progress.total_files,
                    "chunks": progress.total_chunks,
                });
                if !as_json {
                    eprintln!(
                        "{}: {}/{} files, {} chunks",
                        last["phase"].as_str().unwrap_or("indexing"),
                        last["indexed"].as_u64().unwrap_or(0),
                        last["total"].as_u64().unwrap_or(0),
                        last["chunks"].as_u64().unwrap_or(0),
                    );
                }
                if last["phase"] == "done" {
                    break;
                }
            }
            let join = handle.join().map_err(|_| anyhow::anyhow!("Reindex thread panicked"))?;
            join?;
            print_output(&last, as_json)?;
        }
        Commands::Backfill => {
            let db = Arc::new(Mutex::new(Database::open(&config.db_path)?));
            let model_id = &config.model_id;
            let model_cache = &config.model_cache_dir;
            let embedder = rindex::embedding::Embedder::load(
                model_cache,
                model_id,
                Some("https://hf-mirror.com"),
            )?;
            eprintln!("Model loaded, backfilling missing embeddings...");
            rindex::indexer::backfill_embeddings(&db, &embedder, true)?;
            let out = serde_json::json!({"backfill": "complete"});
            print_output(&out, as_json)?;
        }
        Commands::Setup { claude, opencode, cursor } => {
            let cwd = std::env::current_dir()?;
            let bin_str = if cfg!(target_os = "windows") { "rindex.exe" } else { "rindex" };

            let mut agents: Vec<&str> = Vec::new();
            if claude { agents.push("claude"); }
            if opencode { agents.push("opencode"); }
            if cursor { agents.push("cursor"); }

            if agents.is_empty() {
                // No flags: project-level setup only
                let skill_dir = cwd.join(".claude").join("skills").join("rindex");
                std::fs::create_dir_all(&skill_dir)?;
                let skill_md = include_str!("../../plugin/skills/rindex/SKILL.md");
                std::fs::write(skill_dir.join("SKILL.md"), skill_md)?;
                eprintln!("SKILL.md → .claude/skills/rindex/");

                let ignore_path = cwd.join(".llm-index-ignore");
                if !ignore_path.exists() {
                    std::fs::write(&ignore_path, "# Add patterns to exclude from rindex (one per line)\n# These work like .gitignore\n")?;
                    eprintln!(".llm-index-ignore created");
                }
            } else {
                // User-level MCP registration for each requested agent
                for agent in &agents {
                    match *agent {
                        "claude" => setup_claude_mcp(&bin_str)?,
                        "opencode" => setup_opencode_mcp(&bin_str)?,
                        "cursor" => setup_cursor_mcp(&bin_str)?,
                        _ => {}
                    }
                }

                // Also install project-level skill so the agent can use it immediately
                let skill_dir = cwd.join(".claude").join("skills").join("rindex");
                std::fs::create_dir_all(&skill_dir)?;
                let skill_md = include_str!("../../plugin/skills/rindex/SKILL.md");
                std::fs::write(skill_dir.join("SKILL.md"), skill_md)?;
                eprintln!("SKILL.md → .claude/skills/rindex/");

                let ignore_path = cwd.join(".llm-index-ignore");
                if !ignore_path.exists() {
                    std::fs::write(&ignore_path, "# Add patterns to exclude from rindex (one per line)\n# These work like .gitignore\n")?;
                    eprintln!(".llm-index-ignore created");
                }
            }

            let out = serde_json::json!({"setup": "ok", "project": cwd.to_string_lossy(), "agents": agents});
             print_output(&out, as_json)?;
        }
    }

    Ok(())
}

fn setup_claude_mcp(bin_str: &str) -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    let config_path = home.join(".claude.json");
    let mut config: serde_json::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    config.as_object_mut()
        .ok_or_else(|| anyhow::anyhow!(".claude.json is not an object"))?
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("mcpServers is not an object"))?
        .insert("rindex".to_string(), serde_json::json!({
            "command": bin_str
        }));
    std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    eprintln!("Claude Code MCP → ~/.claude.json");
    Ok(())
}

fn setup_opencode_mcp(bin_str: &str) -> Result<()> {
    let config_dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot find config directory"))?;
    let config_path = config_dir.join("opencode").join("opencode.jsonc");
    std::fs::create_dir_all(config_path.parent().unwrap())?;
    let mut config: serde_json::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    config.as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("opencode.jsonc is not an object"))?
        .entry("mcp")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("mcp is not an object"))?
        .insert("rindex".to_string(), serde_json::json!({
            "type": "local",
            "command": bin_str,
            "enabled": true
        }));
    std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    eprintln!("opencode MCP → {}", config_path.display());
    Ok(())
}

fn setup_cursor_mcp(bin_str: &str) -> Result<()> {
    let config_path = if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA")
            .map_err(|_| anyhow::anyhow!("APPDATA not set"))?;
        std::path::PathBuf::from(&appdata)
            .join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("saoudrizwan.claude-dev")
            .join("settings")
            .join("cline_mcp_settings.json")
    } else {
        let home = dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
        home.join(".cursor")
            .join("mcp.json")
    };
    std::fs::create_dir_all(config_path.parent().unwrap())?;
    let mut config: serde_json::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };
    config.as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("cline_mcp_settings.json is not an object"))?
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("mcpServers is not an object"))?
        .insert("rindex".to_string(), serde_json::json!({
            "command": bin_str
        }));
    std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    eprintln!("Cursor MCP → {}", config_path.display());
    Ok(())
}
