pub mod chunker;
pub mod parser;
pub mod walker;

use crate::db::Database;
use crate::db::queries;
use crate::embedding::Embedder;
use crate::ignore::IgnoreEngine;
use crate::indexer::chunker::chunk_file;
use crate::indexer::walker::FileWalker;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug, Clone)]
pub struct IndexProgress {
    pub total_files: usize,
    pub indexed_files: usize,
    pub total_chunks: usize,
    pub phase: String,
}

pub fn compute_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}

/// Run a full index of the project. Progress is sent via the channel.
pub fn index_project(
    db: Arc<Mutex<Database>>,
    embedder: Option<Arc<Embedder>>,
    ignore: Arc<IgnoreEngine>,
    root: &Path,
) -> (thread::JoinHandle<Result<()>>, mpsc::Receiver<IndexProgress>) {
    let (tx, rx) = mpsc::channel();
    let root_path = root.to_path_buf();

    let handle = thread::spawn(move || {
        let walker = FileWalker::new(&ignore);

        // Phase 1: Scan files
        tx.send(IndexProgress { total_files: 0, indexed_files: 0, total_chunks: 0, phase: "scanning".to_string() }).ok();
        let files = walker.walk(&root_path)?;
        let total = files.len();
        tx.send(IndexProgress { total_files: total, indexed_files: 0, total_chunks: 0, phase: "indexing".to_string() }).ok();

        // Phase 2: Index each file
        let mut indexed = 0;
        for entry in &files {
            if let Err(e) = index_single_file(&db, embedder.as_deref(), entry) {
                tracing::warn!("Failed to index {}: {}", entry.relative_path, e);
            }
            indexed += 1;
            if indexed % 5 == 0 {
                let chunk_count = db.lock()
                    .map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?
                    .conn()
                    .map(|c| c.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get::<_, i64>(0)).unwrap_or(0))
                    .unwrap_or(0) as usize;
                tx.send(IndexProgress { total_files: total, indexed_files: indexed, total_chunks: chunk_count, phase: "indexing".to_string() }).ok();
            }
        }

        // Update project stats
        let chunk_count_final = db.lock()
            .map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?
            .conn()
            .map(|c| c.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get::<_, i64>(0)).unwrap_or(0))
            .unwrap_or(0) as usize;
        let root_str = root_path.to_string_lossy().to_string();
        let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;
        queries::update_project_stats(&db_guard, &root_str, total as i64, chunk_count_final as i64)?;

        tx.send(IndexProgress { total_files: total, indexed_files: total, total_chunks: chunk_count_final, phase: "done".to_string() }).ok();
        Ok(())
    });

    (handle, rx)
}

fn index_single_file(db: &Arc<Mutex<Database>>, embedder: Option<&Embedder>, entry: &crate::indexer::walker::FileEntry) -> Result<()> {
    let content = std::fs::read_to_string(&entry.path)?;
    let hash = compute_hash(&content);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;

    let db_guard = db.lock().map_err(|e| anyhow::anyhow!("Mutex poisoned: {}", e))?;

    // Check if file changed
    if let Some(existing) = queries::get_file(&db_guard, &entry.relative_path)? {
        if existing.hash == hash {
            return Ok(()); // No change
        }
    }

    // Delete old chunks
    queries::delete_chunks_for_file(&db_guard, &entry.relative_path)?;

    // Chunk
    let chunks = chunk_file(&content, &entry.language)?;

    // Store file
    queries::upsert_file(&db_guard, &entry.relative_path, &hash, entry.size as i64, entry.mtime as i64, &entry.language, now)?;

    // Store chunks with embeddings
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
            let vec = emb.embed(&content_for_embed)?;
            store_embedding(&db_guard, chunk_id, &vec)?;
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
