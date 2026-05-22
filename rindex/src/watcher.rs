use crate::db::Database;
use crate::ignore::IgnoreEngine;
use crate::indexer::walker::FileEntry;
use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Debounce window: events within this window are coalesced into one batch
const DEBOUNCE_MS: u64 = 200;

/// File watcher that monitors a project directory and emits debounced change notifications
#[allow(dead_code)]
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    change_rx: mpsc::Receiver<Vec<PathBuf>>,
}

impl FileWatcher {
    /// Start watching `root` for file changes.
    /// Changed paths are debounced and sent through the returned receiver.
    pub fn start(root: &Path) -> Result<mpsc::Receiver<Vec<PathBuf>>> {
        let (event_tx, event_rx) = mpsc::channel();
        let (change_tx, change_rx) = mpsc::channel();

        let mut watcher: RecommendedWatcher =
            Watcher::new(event_tx, notify::Config::default())?;
        watcher.watch(root, RecursiveMode::Recursive)?;

        // Background thread: debounce raw events and emit batches
        std::thread::Builder::new()
            .name("rindex-watcher".into())
            .spawn(move || {
                let mut pending: HashSet<PathBuf> = HashSet::new();
                let mut last_flush = Instant::now();

                loop {
                    // Try to receive events with a timeout matching the debounce window
                    match event_rx.recv_timeout(Duration::from_millis(DEBOUNCE_MS)) {
                        Ok(Ok(event)) => {
                            // Extract file paths from the event
                            for path in event.paths {
                                if path.is_file() {
                                    pending.insert(path);
                                }
                            }
                        }
                        Ok(Err(_)) => {}
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            // Timeout means no new events — flush if we have pending
                            if !pending.is_empty() && last_flush.elapsed() >= Duration::from_millis(DEBOUNCE_MS) {
                                let batch: Vec<PathBuf> = pending.drain().collect();
                                let _ = change_tx.send(batch);
                                last_flush = Instant::now();
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            })?;

        Ok(change_rx)
    }
}

/// Process a batch of changed files: re-index each file incrementally
pub fn process_changes(
    changes: &[PathBuf],
    db: &Arc<Mutex<Database>>,
    ignore: &IgnoreEngine,
    root: &Path,
) {
    // Invalidate search cache on any file change
    crate::search::invalidate_cache();

    for path in changes {
        // Determine relative path
        let relative = match path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };

        // Check if this file should be indexed
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        if !ignore.should_index(&relative, size, &ext) {
            continue;
        }

        // Determine language from extension
        let language = ext_to_language(&ext);

        let entry = FileEntry {
            path: path.to_path_buf(),
            relative_path: relative.clone(),
            size,
            mtime: std::fs::metadata(path)
                .and_then(|m| m.modified())
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
                .unwrap_or(0),
            language,
        };

        tracing::debug!("File changed, re-indexing: {}", relative);
        if let Err(e) = crate::indexer::index_single_file_public(db, None, &entry) {
            tracing::warn!("Failed to re-index {}: {}", relative, e);
        }
    }
}

fn ext_to_language(ext: &str) -> String {
    match ext {
        "rs" => "rust", "py" => "python",
        "js" | "jsx" => "javascript", "ts" | "tsx" => "typescript",
        "go" => "go", "c" | "h" => "c", "cpp" | "hpp" | "cc" => "cpp",
        "java" => "java", "kt" | "kts" => "kotlin", "swift" => "swift",
        "rb" => "ruby", "php" => "php", "pl" | "pm" => "perl",
        "lua" => "lua", "toml" => "toml", "json" => "json",
        "yaml" | "yml" => "yaml", "md" => "markdown", "html" => "html",
        "css" => "css", "sh" | "bash" => "shell", "sql" => "sql",
        "vue" => "vue", "svelte" => "svelte", "dockerfile" => "dockerfile",
        "gradle" => "gradle",
        _ => ext,
    }.to_string()
}
