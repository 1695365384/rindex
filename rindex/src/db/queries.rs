use crate::db::Database;
use anyhow::Result;
use rusqlite::params;

#[derive(Debug, Clone)]
pub struct FileRecord {
    pub path: String,
    pub hash: String,
    pub size: i64,
    pub mtime: i64,
    pub language: Option<String>,
    pub indexed_at: i64,
}

#[derive(Debug, Clone)]
pub struct ChunkRecord {
    pub id: i64,
    pub file_path: String,
    pub chunk_type: String,
    pub name: Option<String>,
    pub signature: Option<String>,
    pub start_line: i64,
    pub end_line: i64,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ProjectRecord {
    pub id: i64,
    pub root_path: String,
    pub indexed_at: Option<i64>,
    pub file_count: i64,
    pub chunk_count: i64,
}

impl Default for ProjectRecord {
    fn default() -> Self {
        Self { id: 0, root_path: String::new(), indexed_at: None, file_count: 0, chunk_count: 0 }
    }
}

pub fn upsert_file(db: &Database, path: &str, hash: &str, size: i64, mtime: i64, language: &str, now: i64) -> Result<()> {
    let conn = db.conn();
    conn.execute(
        "INSERT INTO files (path, hash, size, mtime, language, indexed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(path) DO UPDATE SET hash=excluded.hash, size=excluded.size, mtime=excluded.mtime, language=excluded.language, indexed_at=excluded.indexed_at",
        params![path, hash, size, mtime, language, now],
    )?;
    Ok(())
}

pub fn get_file(db: &Database, path: &str) -> Result<Option<FileRecord>> {
    let conn = db.conn();
    let mut stmt = conn.prepare("SELECT path, hash, size, mtime, language, indexed_at FROM files WHERE path = ?1")?;
    let mut rows = stmt.query(params![path])?;
    if let Some(row) = rows.next()? {
        Ok(Some(FileRecord { path: row.get(0)?, hash: row.get(1)?, size: row.get(2)?, mtime: row.get(3)?, language: row.get(4)?, indexed_at: row.get(5)? }))
    } else { Ok(None) }
}

pub fn get_all_files(db: &Database) -> Result<Vec<FileRecord>> {
    let conn = db.conn();
    let mut stmt = conn.prepare("SELECT path, hash, size, mtime, language, indexed_at FROM files")?;
    let rows = stmt.query_map([], |row| Ok(FileRecord { path: row.get(0)?, hash: row.get(1)?, size: row.get(2)?, mtime: row.get(3)?, language: row.get(4)?, indexed_at: row.get(5)? }))?;
    let mut files = Vec::new();
    for row in rows { files.push(row?); }
    Ok(files)
}

pub fn delete_file(db: &Database, path: &str) -> Result<()> {
    let conn = db.conn();
    conn.execute("DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_path = ?1)", params![path])?;
    conn.execute("DELETE FROM chunks WHERE file_path = ?1", params![path])?;
    conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
    Ok(())
}

pub fn insert_chunk(db: &Database, file_path: &str, chunk_type: &str, name: Option<&str>, signature: Option<&str>, start_line: i64, end_line: i64, content: &str) -> Result<i64> {
    let conn = db.conn();
    conn.execute("INSERT INTO chunks (file_path, chunk_type, name, signature, start_line, end_line, content) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![file_path, chunk_type, name, signature, start_line, end_line, content])?;
    Ok(conn.last_insert_rowid())
}

pub fn get_chunks_for_file(db: &Database, file_path: &str) -> Result<Vec<ChunkRecord>> {
    let conn = db.conn();
    let mut stmt = conn.prepare("SELECT id, file_path, chunk_type, name, signature, start_line, end_line, content FROM chunks WHERE file_path = ?1 ORDER BY start_line")?;
    let rows = stmt.query_map(params![file_path], |row| Ok(ChunkRecord { id: row.get(0)?, file_path: row.get(1)?, chunk_type: row.get(2)?, name: row.get(3)?, signature: row.get(4)?, start_line: row.get(5)?, end_line: row.get(6)?, content: row.get(7)? }))?;
    let mut chunks = Vec::new();
    for row in rows { chunks.push(row?); }
    Ok(chunks)
}

pub fn delete_chunks_for_file(db: &Database, file_path: &str) -> Result<()> {
    let conn = db.conn();
    // Delete FTS entries for this file's chunks first (FTS has no foreign keys)
    conn.execute(
        "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE file_path = ?1)",
        params![file_path],
    )?;
    conn.execute("DELETE FROM chunks WHERE file_path = ?1", params![file_path])?;
    Ok(())
}

/// Insert or replace an FTS5 entry for a chunk (uses chunk id as FTS rowid)
pub fn insert_chunk_fts(db: &Database, chunk_id: i64, content: &str, name: Option<&str>, file_path: &str) -> Result<()> {
    let conn = db.conn();
    conn.execute(
        "INSERT OR REPLACE INTO chunks_fts (rowid, content, name, file_path) VALUES (?1, ?2, ?3, ?4)",
        params![chunk_id, content, name, file_path],
    )?;
    Ok(())
}

/// Rebuild the entire FTS index from the chunks table (used after migration or reindex)
pub fn rebuild_fts_index(db: &Database) -> Result<usize> {
    let conn = db.conn();
    // Clear existing FTS index
    conn.execute("DELETE FROM chunks_fts", [])?;
    // Re-populate from chunks table
    let mut stmt = conn.prepare("SELECT id, content, name, file_path FROM chunks")?;
    let rows: Vec<(i64, String, Option<String>, String)> = stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })?.filter_map(|r| r.ok()).collect();
    let count = rows.len();
    for (id, content, name, file_path) in &rows {
        conn.execute(
            "INSERT INTO chunks_fts (rowid, content, name, file_path) VALUES (?1, ?2, ?3, ?4)",
            params![id, content, name, file_path],
        )?;
    }
    Ok(count)
}

pub fn get_or_create_project(db: &Database, root_path: &str) -> Result<ProjectRecord> {
    let conn = db.conn();
    conn.execute("INSERT OR IGNORE INTO project (root_path) VALUES (?1)", params![root_path])?;
    let mut stmt = conn.prepare("SELECT id, root_path, indexed_at, file_count, chunk_count FROM project WHERE root_path = ?1")?;
    let record = stmt.query_row(params![root_path], |row| Ok(ProjectRecord { id: row.get(0)?, root_path: row.get(1)?, indexed_at: row.get(2)?, file_count: row.get(3)?, chunk_count: row.get(4)? }))?;
    Ok(record)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Observation {
    pub id: i64,
    pub project_root: String,
    pub kind: String,
    pub content: String,
    pub created_at: i64,
}

pub fn insert_observation(db: &Database, project_root: &str, kind: &str, content: &str) -> Result<i64> {
    let conn = db.conn();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    conn.execute(
        "INSERT INTO observations (project_root, kind, content, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![project_root, kind, content, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_recent_observations(db: &Database, project_root: &str, limit: usize, kind: Option<&str>) -> Result<Vec<Observation>> {
    let conn = db.conn();
    let sql = if let Some(_k) = kind {
        "SELECT id, project_root, kind, content, created_at FROM observations WHERE project_root = ?1 AND kind = ?2 ORDER BY created_at DESC LIMIT ?3"
    } else {
        "SELECT id, project_root, kind, content, created_at FROM observations WHERE project_root = ?1 ORDER BY created_at DESC LIMIT ?2"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows: Vec<Observation> = if let Some(k) = kind {
        stmt.query_map(params![project_root, k, limit as i64], |row| {
            Ok(Observation { id: row.get(0)?, project_root: row.get(1)?, kind: row.get(2)?, content: row.get(3)?, created_at: row.get(4)? })
        })?.filter_map(|r| r.ok()).collect()
    } else {
        stmt.query_map(params![project_root, limit as i64], |row| {
            Ok(Observation { id: row.get(0)?, project_root: row.get(1)?, kind: row.get(2)?, content: row.get(3)?, created_at: row.get(4)? })
        })?.filter_map(|r| r.ok()).collect()
    };
    Ok(rows)
}

pub fn count_observations(db: &Database, project_root: &str) -> Result<i64> {
    let conn = db.conn();
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM observations WHERE project_root = ?1",
        params![project_root],
        |row| row.get(0),
    )?)
}

pub fn update_project_stats(db: &Database, root_path: &str, file_count: i64, chunk_count: i64) -> Result<()> {
    let conn = db.conn();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    conn.execute(
        "INSERT INTO project (root_path, file_count, chunk_count, indexed_at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(root_path) DO UPDATE SET file_count=excluded.file_count, chunk_count=excluded.chunk_count, indexed_at=excluded.indexed_at",
        params![root_path, file_count, chunk_count, now],
    )?;
    Ok(())
}
