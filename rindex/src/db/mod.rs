pub mod migrations;
pub mod queries;

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open database at {:?}", path))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self { conn: Mutex::new(conn) };
        db.run_migrations()?;
        Ok(db)
    }

    /// Safely lock the database connection, recovering from poison
    pub fn conn(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn.lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))
    }

    pub fn run_migrations(&self) -> Result<()> {
        let conn = self.conn()?;
        migrations::run(&conn)?;
        Ok(())
    }

    /// Open an in-memory database (used in integration tests)
    pub fn open_temp() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let db = Self { conn: Mutex::new(conn) };
        db.run_migrations()?;
        Ok(db)
    }
}
