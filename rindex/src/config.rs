use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use serde::Deserialize;
use std::fs;

#[derive(Parser, Debug)]
#[command(name = "rindex", version, about = "Local file index MCP server with semantic search")]
pub struct Cli {
    /// Project root directory to index (default: current directory)
    #[arg(short = 'p', long = "path")]
    pub project_root: Option<PathBuf>,

    /// Path to SQLite database (default: ~/.local/share/rindex/rindex.db)
    #[arg(long = "db")]
    pub db_path: Option<PathBuf>,

    /// Config file path (default: search rindex.toml in project root)
    #[arg(long = "config")]
    pub config: Option<PathBuf>,

    /// Skip loading the embedding model (text-only search)
    #[arg(long = "no-model")]
    pub no_model: bool,

    /// Maximum file size in bytes to index (default: 1MB)
    #[arg(long = "max-size")]
    pub max_file_size: Option<u64>,

    /// Embedding model HuggingFace ID
    #[arg(long = "model-id")]
    pub model_id: Option<String>,

    /// Default search result limit
    #[arg(long = "search-limit")]
    pub search_limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ConfigFile {
    pub project_root: Option<String>,
    pub db_path: Option<String>,
    pub max_file_size: Option<u64>,
    pub model_id: Option<String>,
    pub model_cache_dir: Option<String>,
    pub embedding_batch_size: Option<usize>,
    pub default_search_limit: Option<usize>,
    pub watcher_debounce_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub project_root: PathBuf,
    pub db_path: PathBuf,
    pub model_cache_dir: PathBuf,
    pub max_file_size: u64,
    pub model_id: String,
    pub no_model: bool,
    pub embedding_batch_size: usize,
    pub default_search_limit: usize,
    pub watcher_debounce_ms: u64,
}

impl Config {
    /// Load config from CLI args + config file + defaults (merged in that priority order)
    pub fn load() -> Result<Self> {
        let cli = Cli::parse();

        // Determine project root and config file path
        let project_root = cli.project_root.clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));

        // Load config file if specified, or search for rindex.toml
        let config_file: ConfigFile = cli.config.as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .or_else(|| {
                // Try project root rindex.toml, then ~/.config/rindex/config.toml
                let candidates = [
                    Some(project_root.join("rindex.toml")),
                    dirs::config_dir().map(|d| d.join("rindex/config.toml")),
                ];
                candidates.iter().flatten().find_map(|p| fs::read_to_string(p).ok())
            })
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default();

        let cache_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rindex");

        Ok(Self {
            project_root,
            db_path: cli.db_path
                .or_else(|| config_file.db_path.map(PathBuf::from))
                .unwrap_or_else(|| cache_dir.join("rindex.db")),
            model_cache_dir: config_file.model_cache_dir
                .map(PathBuf::from)
                .unwrap_or_else(|| cache_dir.join("models")),
            max_file_size: cli.max_file_size
                .or(config_file.max_file_size)
                .unwrap_or(1_048_576),
            model_id: cli.model_id
                .or(config_file.model_id)
                .unwrap_or_else(|| "BAAI/bge-small-en-v1.5".to_string()),
            no_model: cli.no_model,
            embedding_batch_size: config_file.embedding_batch_size.unwrap_or(32),
            default_search_limit: cli.search_limit
                .or(config_file.default_search_limit)
                .unwrap_or(10),
            watcher_debounce_ms: config_file.watcher_debounce_ms.unwrap_or(500),
        })
    }
}
