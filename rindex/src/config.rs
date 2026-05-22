use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub project_root: PathBuf,
    pub db_path: PathBuf,
    pub model_cache_dir: PathBuf,
    pub max_file_size: u64,
    pub model_id: String,
    pub embedding_batch_size: usize,
    pub default_search_limit: usize,
    pub watcher_debounce_ms: u64,
}

impl Config {
    pub fn from_project_root(root: &std::path::Path) -> Self {
        let cache_dir = dirs_or_default();
        Self {
            project_root: root.to_path_buf(),
            db_path: cache_dir.join("rindex.db"),
            model_cache_dir: cache_dir.join("models"),
            max_file_size: 1_048_576,
            model_id: "BAAI/bge-small-en-v1.5".to_string(),
            embedding_batch_size: 32,
            default_search_limit: 10,
            watcher_debounce_ms: 500,
        }
    }
}

fn dirs_or_default() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rindex")
}
