use anyhow::Result;
use rusqlite::Connection;

pub fn run(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS project (
            id INTEGER PRIMARY KEY,
            root_path TEXT NOT NULL UNIQUE,
            indexed_at INTEGER,
            file_count INTEGER DEFAULT 0,
            chunk_count INTEGER DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS files (
            path TEXT PRIMARY KEY,
            hash TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime INTEGER NOT NULL,
            language TEXT,
            indexed_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS chunks (
            id INTEGER PRIMARY KEY,
            file_path TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
            chunk_type TEXT NOT NULL,
            name TEXT,
            signature TEXT,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            content TEXT NOT NULL,
            embedding BLOB
        );

        CREATE TABLE IF NOT EXISTS file_events (
            path TEXT PRIMARY KEY,
            event_type TEXT NOT NULL,
            detected_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS ignore_patterns (
            pattern TEXT PRIMARY KEY,
            source TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_chunks_file_path ON chunks(file_path);
        CREATE INDEX IF NOT EXISTS idx_chunks_name ON chunks(name);
        CREATE INDEX IF NOT EXISTS idx_files_language ON files(language);
        "
    )?;
    Ok(())
}
