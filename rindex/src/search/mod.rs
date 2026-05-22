use crate::db::Database;
use crate::embedding::Embedder;
use anyhow::Result;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub file_path: String,
    pub chunk_type: String,
    pub name: Option<String>,
    pub start_line: i64,
    pub end_line: i64,
    pub snippet: String,
    pub score: f64,
}

pub struct Searcher<'a> {
    db: &'a Database,
    embedder: Option<&'a Embedder>,
}

impl<'a> Searcher<'a> {
    pub fn new(db: &'a Database, embedder: Option<&'a Embedder>) -> Self {
        Self { db, embedder }
    }

    /// Search by symbol name (LIKE query)
    pub fn search_symbol(&self, name: &str, chunk_type: Option<&str>) -> Result<Vec<SearchResult>> {
        let conn = self.db.conn()?;
        let pattern = format!("%{}%", name);

        let rows: Vec<SearchResult> = if let Some(ctype) = chunk_type {
            let mut stmt = conn.prepare(
                "SELECT file_path, chunk_type, name, start_line, end_line, content
                 FROM chunks WHERE name LIKE ?1 AND chunk_type = ?2
                 ORDER BY
                   CASE WHEN name = ?3 THEN 0 ELSE 1 END,
                   LENGTH(name),
                   name
                 LIMIT 20"
            )?;
            let rows: Vec<SearchResult> = stmt.query_map(
                rusqlite::params![pattern, ctype, name],
                |row| Ok(SearchResult {
                    file_path: row.get(0)?, chunk_type: row.get(1)?,
                    name: row.get(2)?, start_line: row.get(3)?,
                    end_line: row.get(4)?, snippet: row.get(5)?,
                    score: 1.0,
                })
            )?.filter_map(|r| r.ok()).collect();
            rows
        } else {
            let mut stmt = conn.prepare(
                "SELECT file_path, chunk_type, name, start_line, end_line, content
                 FROM chunks WHERE name LIKE ?1
                 ORDER BY
                   CASE WHEN name = ?2 THEN 0 ELSE 1 END,
                   LENGTH(name),
                   name
                 LIMIT 20"
            )?;
            let rows: Vec<SearchResult> = stmt.query_map(
                rusqlite::params![pattern, name],
                |row| Ok(SearchResult {
                    file_path: row.get(0)?, chunk_type: row.get(1)?,
                    name: row.get(2)?, start_line: row.get(3)?,
                    end_line: row.get(4)?, snippet: row.get(5)?,
                    score: 1.0,
                })
            )?.filter_map(|r| r.ok()).collect();
            rows
        };

        Ok(rows)
    }

    /// Semantic search using embeddings (brute-force cosine similarity)
    pub fn semantic_search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let query_vec = match self.embedder {
            Some(emb) => emb.embed(query)?,
            None => return self.search_symbol(query, None),
        };

        let conn = self.db.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, file_path, chunk_type, name, start_line, end_line, content, embedding
             FROM chunks WHERE embedding IS NOT NULL"
        )?;

        let rows = stmt.query_map([], |row| {
            let blob: Vec<u8> = row.get(7)?;
            let vec: Vec<f32> = blob.chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            Ok((SearchResult {
                file_path: row.get(1)?,
                chunk_type: row.get(2)?,
                name: row.get(3)?,
                start_line: row.get(4)?,
                end_line: row.get(5)?,
                snippet: row.get(6)?,
                score: 0.0,
            }, vec))
        })?;

        let mut scored: Vec<(SearchResult, f64)> = rows
            .filter_map(|r| r.ok())
            .map(|(mut result, vec)| {
                let dot: f32 = query_vec.iter().zip(vec.iter()).map(|(a, b)| a * b).sum();
                result.score = dot as f64;
                (result, dot as f64)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored.into_iter().take(limit).map(|(r, _)| r).collect())
    }
}
