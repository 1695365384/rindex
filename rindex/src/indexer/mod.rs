pub mod chunker;
pub mod parser;
pub mod walker;

use crate::db::Database;
use crate::db::queries;
use crate::embedding::Embedder;
use crate::ignore::IgnoreEngine;
use crate::indexer::chunker::chunk_file;
use crate::indexer::walker::FileWalker;
use anyhow::{Context, Result};
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Number of files per parallel batch
const BATCH_SIZE: usize = 20;

/// Pause between batches to let the MCP event loop process requests
const BATCH_YIELD_MS: u64 = 2;

#[derive(Debug, Clone)]
pub struct IndexProgress {
    pub total_files: usize,
    pub indexed_files: usize,
    pub total_chunks: usize,
    pub phase: String,
}

/// Shared atomic state for progressive indexing progress queries
pub struct IndexState {
    pub total: AtomicUsize,
    pub indexed: AtomicUsize,
    pub chunks: AtomicUsize,
    pub running: AtomicBool,
    pub phase: Mutex<String>,
}

impl IndexState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            total: AtomicUsize::new(0),
            indexed: AtomicUsize::new(0),
            chunks: AtomicUsize::new(0),
            running: AtomicBool::new(false),
            phase: Mutex::new("idle".to_string()),
        })
    }

    pub fn snapshot(&self) -> IndexProgress {
        IndexProgress {
            total_files: self.total.load(Ordering::Acquire),
            indexed_files: self.indexed.load(Ordering::Acquire),
            total_chunks: self.chunks.load(Ordering::Acquire),
            phase: self.phase.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        }
    }
}

pub fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Run a full index of the project in progressive batches.
/// The indexer processes files in small batches and yields between them,
/// allowing the MCP event loop to handle requests promptly.
pub fn index_project(
    db: Arc<Mutex<Database>>,
    embedder: Option<Arc<Embedder>>,
    ignore: Arc<IgnoreEngine>,
    root: &Path,
    state: Option<Arc<IndexState>>,
) -> (thread::JoinHandle<Result<()>>, mpsc::Receiver<IndexProgress>) {
    let (tx, rx) = mpsc::channel();
    let root_path = root.to_path_buf();
    let state = state.unwrap_or_else(IndexState::new);

    let handle = thread::spawn(move || {
        state.running.store(true, Ordering::Release);

        // Phase 1: Scan files
        *state.phase.lock().unwrap_or_else(|e| e.into_inner()) = "scanning".to_string();
        tx.send(IndexProgress { total_files: 0, indexed_files: 0, total_chunks: 0, phase: "scanning".to_string() }).ok();

        let walker = FileWalker::new(&ignore);
        let files = walker.walk(&root_path)?;
        let total = files.len();
        state.total.store(total, Ordering::Release);

        *state.phase.lock().unwrap_or_else(|e| e.into_inner()) = "indexing".to_string();
        tx.send(IndexProgress { total_files: total, indexed_files: 0, total_chunks: 0, phase: "indexing".to_string() }).ok();

        // Phase 2: Progressive parallel batch indexing
        let mut indexed = 0;
        for batch in files.chunks(BATCH_SIZE) {
            // Yield between batches so MCP loop can process requests
            thread::sleep(Duration::from_millis(BATCH_YIELD_MS));

            // Process files in parallel within each batch
            batch.par_iter().for_each(|entry| {
                if let Err(e) = index_single_file(&db, embedder.as_deref(), entry) {
                    tracing::warn!("Failed to index {}: {}", entry.relative_path, e);
                }
            });
            indexed += batch.len();
            state.indexed.store(indexed, Ordering::Release);

            // Update chunk count and report progress
            let chunk_count = count_chunks(&db);
            state.chunks.store(chunk_count, Ordering::Release);
            tx.send(IndexProgress {
                total_files: total, indexed_files: indexed, total_chunks: chunk_count,
                phase: "indexing".to_string(),
            }).ok();
        }

        // Phase 3: Done
        let chunk_count_final = count_chunks(&db);
        state.chunks.store(chunk_count_final, Ordering::Release);
        let root_str = root_path.to_string_lossy().to_string();
        let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
        queries::update_project_stats(&db_guard, &root_str, total as i64, chunk_count_final as i64)?;
        drop(db_guard);

        *state.phase.lock().unwrap_or_else(|e| e.into_inner()) = "done".to_string();
        state.running.store(false, Ordering::Release);
        tx.send(IndexProgress {
            total_files: total, indexed_files: total, total_chunks: chunk_count_final,
            phase: "done".to_string(),
        }).ok();

        Ok(())
    });

    (handle, rx)
}

fn count_chunks(db: &Arc<Mutex<Database>>) -> usize {
    let guard = match db.lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    guard.conn().query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get::<_, i64>(0))
        .unwrap_or(0) as usize
}

fn index_single_file(db: &Arc<Mutex<Database>>, embedder: Option<&Embedder>, entry: &crate::indexer::walker::FileEntry) -> Result<()> {
    let content = std::fs::read_to_string(&entry.path)
        .with_context(|| format!("Failed to read {:?}", entry.path))?;
    let hash = compute_hash(&content);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;

    // Chunk the file and compute embeddings OUTSIDE the lock
    let chunks = chunk_file(&content, &entry.language)?;

    // Quick hash check — only hold lock for the check
    {
        let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
        if let Some(existing) = queries::get_file(&db_guard, &entry.relative_path)? {
            if existing.hash == hash {
                return Ok(());
            }
        }
    }

    // All DB writes under one lock acquisition
    let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;

    queries::delete_chunks_for_file(&db_guard, &entry.relative_path)?;
    queries::upsert_file(&db_guard, &entry.relative_path, &hash, entry.size as i64, entry.mtime as i64, &entry.language, now)?;

    for chunk in &chunks {
        let chunk_id = queries::insert_chunk(
            &db_guard, &entry.relative_path, &chunk.chunk_type,
            chunk.name.as_deref(), None,
            chunk.start_line as i64, chunk.end_line as i64, &chunk.content,
        )?;

        queries::insert_chunk_fts(
            &db_guard, chunk_id, &chunk.content,
            chunk.name.as_deref(), &entry.relative_path,
        )?;

        if let Some(emb) = embedder {
            let content_for_embed = chunk.name.as_deref()
                .map(|n| format!("{}: {}", n, chunk.content))
                .unwrap_or_else(|| chunk.content.clone());
            match emb.embed(&content_for_embed) {
                Ok(vec) => store_embedding(&db_guard, chunk_id, &vec)?,
                Err(e) => tracing::warn!("Embedding failed for chunk {}: {}", chunk_id, e),
            }
        }
    }

    Ok(())
}

fn store_embedding(db: &Database, chunk_id: i64, vec: &[f32]) -> Result<()> {
    let conn = db.conn();
    let bytes: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
    conn.execute("UPDATE chunks SET embedding = ?1 WHERE id = ?2", rusqlite::params![bytes, chunk_id])?;
    Ok(())
}

/// Backfill embeddings for all chunks.
/// If `force` is true, clears existing embeddings first (e.g. after model upgrade).
/// Called after the model loads lazily to fill in gaps from the initial text-only index.
pub fn backfill_embeddings(db: &Arc<Mutex<Database>>, embedder: &Embedder, force: bool) -> Result<()> {
    let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
    let conn = db_guard.conn();

    if force {
        conn.execute("UPDATE chunks SET embedding = NULL", [])?;
        tracing::info!("Cleared existing embeddings for full re-backfill");
    }

    let mut stmt = conn.prepare(
        "SELECT id, content, name FROM chunks WHERE embedding IS NULL ORDER BY id"
    )?;
    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let content: String = row.get(1)?;
        let name: Option<String> = row.get(2)?;
        Ok((id, content, name))
    })?;

    let mut count = 0;
    for row in rows {
        let (id, content, name) = row?;
        let text = name.as_ref()
            .map(|n| format!("{}: {}", n, content))
            .unwrap_or(content);
        match embedder.embed(&text) {
            Ok(vec) => {
                let bytes: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
                conn.execute("UPDATE chunks SET embedding = ?1 WHERE id = ?2",
                    rusqlite::params![bytes, id])?;
                count += 1;
            }
            Err(e) => tracing::warn!("Embedding backfill failed for chunk {}: {}", id, e),
        }
    }

    if count > 0 {
        tracing::info!("Backfilled embeddings for {} chunks", count);
    }
    Ok(())
}

/// Verify index integrity: remove stale entries, count mismatches.
/// Returns (files_removed, files_missing_from_index, total_checked).
pub fn verify_index(
    db: &Arc<Mutex<Database>>,
    ignore: &IgnoreEngine,
    root: &Path,
) -> Result<(usize, usize, usize)> {
    let root_path = root.to_path_buf();
    let walker = FileWalker::new(ignore);

    // Get all indexed files
    let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
    let indexed = queries::get_all_files(&db_guard)?;
    let indexed_paths: std::collections::HashSet<String> = indexed.iter().map(|f| f.path.clone()).collect();
    drop(db_guard);

    // Scan actual files on disk
    let actual_files = walker.walk(&root_path)?;
    let actual_paths: std::collections::HashSet<String> = actual_files.iter().map(|f| f.relative_path.clone()).collect();

    // Remove stale entries (indexed but not on disk)
    let mut removed = 0;
    for path in &indexed_paths {
        if !actual_paths.contains(path.as_str()) {
            let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
            if queries::delete_file(&db_guard, path).is_ok() {
                removed += 1;
            }
        }
    }

    // Count missing (on disk but not indexed)
    let missing = actual_paths.difference(&indexed_paths).count();

    Ok((removed, missing, indexed_paths.len()))
}

/// Public wrapper for incremental re-indexing of a single file (used by watcher)
pub fn index_single_file_public(
    db: &Arc<Mutex<Database>>,
    embedder: Option<&Embedder>,
    entry: &crate::indexer::walker::FileEntry,
) -> Result<()> {
    index_single_file(db, embedder, entry)
}

/// Sync the index with the current state of the filesystem.
/// Only re-indexes changed/new files; removes stale entries.
/// This ensures project memory stays accurate across sessions,
/// even when files changed while rindex was not running.
///
/// Uses mtime+size pre-check to skip unchanged files without reading them.
pub fn sync_project_index(
    db: &Arc<Mutex<Database>>,
    embedder: Option<&Embedder>,
    ignore: &IgnoreEngine,
    root: &Path,
) -> Result<(usize, usize, usize)> {
    let root_path = root.to_path_buf();
    let walker = FileWalker::new(ignore);
    let files = walker.walk(&root_path)?;

    let disk_paths: std::collections::HashSet<String> =
        files.iter().map(|f| f.relative_path.clone()).collect();

    // Build lookup of (mtime, size) from DB for quick pre-check
    let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
    let indexed = queries::get_all_files(&db_guard)?;
    let db_meta: std::collections::HashMap<&str, (i64, i64)> = indexed
        .iter()
        .map(|f| (f.path.as_str(), (f.mtime, f.size)))
        .collect();
    let indexed_paths: std::collections::HashSet<&str> =
        indexed.iter().map(|f| f.path.as_str()).collect();
    drop(db_guard);

    // Phase 1: Re-index new/changed files (quick mtime/size pre-check avoids file read + hash)
    let mut indexed_count = 0;
    for entry in &files {
        // Skip if mtime and size match exactly — file hasn't changed
        if let Some(&(db_mtime, db_size)) = db_meta.get(entry.relative_path.as_str()) {
            if db_mtime == entry.mtime as i64 && db_size == entry.size as i64 {
                continue;
            }
        }
        if let Err(e) = index_single_file(db, embedder, entry) {
            tracing::warn!("Failed to sync {}: {}", entry.relative_path, e);
        }
        indexed_count += 1;
    }

    // Phase 2: Remove stale entries (indexed but no longer on disk)
    let mut removed = 0;
    for path in indexed_paths {
        if !disk_paths.contains(path) {
            let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
            if queries::delete_file(&db_guard, path).is_ok() {
                removed += 1;
            }
        }
    }

    // Phase 3: Update project stats
    let chunk_count = count_chunks(db);
    let root_str = root_path.to_string_lossy().to_string();
    let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
    queries::update_project_stats(&db_guard, &root_str, files.len() as i64, chunk_count as i64)?;

    if indexed_count > 0 || removed > 0 {
        crate::search::invalidate_cache();
        tracing::info!("Index sync: {} re-indexed, {} stale removed (total {} files)",
            indexed_count, removed, files.len());
    } else {
        tracing::info!("Index is current ({} files, no changes detected)", files.len());
    }

    Ok((indexed_count, removed, files.len()))
}
