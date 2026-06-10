use crate::db::Database;
use crate::ignore::{IgnoreConfig, IgnoreEngine};
use crate::indexer::walker::{FileEntry, FileWalker};
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

/// Process a batch of changed files: re-index or remove from index.
/// Automatically reloads ignore rules when `.gitignore` changes.
/// Returns true if the ignore rules were reloaded (caller may want to log it).
pub fn process_changes(
    changes: &[PathBuf],
    db: &Arc<Mutex<Database>>,
    ignore: &mut IgnoreEngine,
    root: &Path,
) -> bool {
    // Invalidate search cache on any file change
    crate::search::invalidate_cache();

    let mut ignore_reloaded = false;

    for path in changes {
        // Determine relative path
        let relative = match path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => continue,
        };

        // Detect .gitignore changes — reload rules and rescan project
        if relative == ".gitignore" || relative.ends_with("/.gitignore") {
            tracing::info!(".gitignore changed, reloading rules and rescanning");
            let new_ignore = rebuild_ignore_engine(root);
            rescan_against_new_rules(db, root, ignore, &new_ignore);
            *ignore = new_ignore;
            ignore_reloaded = true;
            continue;
        }

        // Check if file still exists — if deleted, remove from index
        if !path.exists() {
            tracing::debug!("File deleted, removing from index: {}", relative);
            let db_guard = match db.lock() {
                Ok(g) => g,
                Err(_) => continue,
            };
            let _ = crate::db::queries::delete_file(&db_guard, &relative);
            continue;
        }

        // Check if this file should be indexed
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let size = metadata.len();

        if !ignore.should_index(&relative, size, &ext) {
            continue;
        }

        // Determine language from extension
        let language = crate::indexer::walker::ext_to_language(&ext);

        let entry = FileEntry {
            path: path.to_path_buf(),
            relative_path: relative.clone(),
            size,
            mtime: metadata.modified()
                .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
                .unwrap_or(0),
            language,
        };

        tracing::debug!("File changed, re-indexing: {}", relative);
        if let Err(e) = crate::indexer::index_single_file_public(db, None, &entry) {
            tracing::warn!("Failed to re-index {}: {}", relative, e);
        }
    }

    ignore_reloaded
}

/// After .gitignore changes, remove newly-ignored files and index newly-included files
fn rescan_against_new_rules(
    db: &Arc<Mutex<Database>>,
    root: &Path,
    old_ignore: &IgnoreEngine,
    new_ignore: &IgnoreEngine,
) {
    let walker = FileWalker::new(new_ignore);
    let old_walker = FileWalker::new(old_ignore);

    let new_files = match walker.walk(root) {
        Ok(f) => f,
        Err(_) => return,
    };
    let old_files = match old_walker.walk(root) {
        Ok(f) => f,
        Err(_) => return,
    };

    let new_set: std::collections::HashSet<String> = new_files.iter().map(|f| f.relative_path.clone()).collect();
    let old_set: std::collections::HashSet<String> = old_files.iter().map(|f| f.relative_path.clone()).collect();

    // Remove files that are no longer wanted
    let db_guard = match db.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    for path in &old_set {
        if !new_set.contains(path.as_str()) {
            let _ = crate::db::queries::delete_file(&db_guard, path);
            tracing::debug!("Removed from index per new .gitignore: {}", path);
        }
    }
    drop(db_guard);

    // Index newly included files
    for entry in &new_files {
        if !old_set.contains(&entry.relative_path) {
            tracing::debug!("Indexing new file per .gitignore change: {}", entry.relative_path);
            let _ = crate::indexer::index_single_file_public(db, None, entry);
        }
    }

    tracing::info!("Rescan complete: {} removed, {} added",
        old_set.difference(&new_set).count(),
        new_set.difference(&old_set).count());
}

/// Rebuild the ignore engine from .gitignore files in the project
fn rebuild_ignore_engine(root: &Path) -> IgnoreEngine {
    let cfg = IgnoreConfig::default();
    let mut engine = IgnoreEngine::new(&cfg);

    // Load root .gitignore
    let root_gi = root.join(".gitignore");
    if root_gi.exists() {
        if let Ok(content) = std::fs::read_to_string(&root_gi) {
            for line in content.lines() {
                engine.add_gitignore_pattern(line);
            }
        }
    }

    // Load .gitignore in subdirectories (git supports this)
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let sub_gi = path.join(".gitignore");
                if sub_gi.exists() {
                    if let Ok(content) = std::fs::read_to_string(&sub_gi) {
                        let prefix = path.file_name().unwrap().to_string_lossy();
                        for line in content.lines() {
                            // Prefix subdirectory gitignore patterns
                            let prefixed = format!("{}/{}", prefix, line);
                            engine.add_gitignore_pattern(&prefixed);
                        }
                    }
                }
            }
        }
    }

    engine
}
