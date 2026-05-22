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
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Number of files to index before yielding to other threads
const BATCH_SIZE: usize = 10;

/// Pause between batches to let the MCP event loop process requests
const BATCH_YIELD_MS: u64 = 5;

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

        // Phase 2: Progressive batch indexing
        let mut indexed = 0;
        for batch in files.chunks(BATCH_SIZE) {
            // Yield between batches so MCP loop can process requests
            thread::sleep(Duration::from_millis(BATCH_YIELD_MS));

            for entry in batch {
                if let Err(e) = index_single_file(&db, embedder.as_deref(), entry) {
                    tracing::warn!("Failed to index {}: {}", entry.relative_path, e);
                }
                indexed += 1;
                state.indexed.store(indexed, Ordering::Release);
            }

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
    let conn = match guard.conn() {
        Ok(c) => c,
        Err(_) => return 0,
    };
    conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get::<_, i64>(0))
        .unwrap_or(0) as usize
}

fn index_single_file(db: &Arc<Mutex<Database>>, embedder: Option<&Embedder>, entry: &crate::indexer::walker::FileEntry) -> Result<()> {
    let content = std::fs::read_to_string(&entry.path)
        .with_context(|| format!("Failed to read {:?}", entry.path))?;
    let hash = compute_hash(&content);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;

    let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;

    // Check if file changed (hash diff)
    if let Some(existing) = queries::get_file(&db_guard, &entry.relative_path)? {
        if existing.hash == hash {
            return Ok(()); // No change, skip
        }
    }

    // Delete old chunks
    queries::delete_chunks_for_file(&db_guard, &entry.relative_path)?;

    // Chunk the file
    let chunks = chunk_file(&content, &entry.language)?;

    // Store file record
    queries::upsert_file(&db_guard, &entry.relative_path, &hash, entry.size as i64, entry.mtime as i64, &entry.language, now)?;

    // Store chunks (with embeddings if model is loaded)
    for chunk in &chunks {
        let chunk_id = queries::insert_chunk(
            &db_guard, &entry.relative_path, &chunk.chunk_type,
            chunk.name.as_deref(), None,
            chunk.start_line as i64, chunk.end_line as i64, &chunk.content,
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
    let conn = db.conn()?;
    let bytes: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();
    conn.execute("UPDATE chunks SET embedding = ?1 WHERE id = ?2", rusqlite::params![bytes, chunk_id])?;
    Ok(())
}

/// Backfill embeddings for all chunks that don't have them yet.
/// Called after the model loads lazily to fill in gaps from the initial text-only index.
pub fn backfill_embeddings(db: &Arc<Mutex<Database>>, embedder: &Embedder) -> Result<()> {
    let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
    let conn = db_guard.conn()?;

    // Find all chunks without embeddings
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

/// Public wrapper for incremental re-indexing of a single file (used by watcher)
pub fn index_single_file_public(
    db: &Arc<Mutex<Database>>,
    embedder: Option<&Embedder>,
    entry: &crate::indexer::walker::FileEntry,
) -> Result<()> {
    index_single_file(db, embedder, entry)
}
