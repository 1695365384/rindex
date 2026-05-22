use crate::db::Database;
use crate::embedding::Embedder;
use anyhow::Result;
use lru::LruCache;
use std::num::NonZeroUsize;

/// Default number of search results to cache
const CACHE_SIZE: usize = 64;

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

/// LRU cache keyed by normalized query string
type SearchCache = std::sync::Mutex<LruCache<String, Vec<SearchResult>>>;

/// Semaphore to avoid redundant embedding computation
static CACHE: once_cell::sync::Lazy<SearchCache> =
    once_cell::sync::Lazy::new(|| {
        std::sync::Mutex::new(LruCache::new(NonZeroUsize::new(CACHE_SIZE).unwrap()))
    });

/// Invalidate the search cache (called when files change)
pub fn invalidate_cache() {
    if let Ok(mut cache) = CACHE.lock() {
        cache.clear();
        tracing::debug!("Search cache invalidated");
    }
}

pub struct Searcher<'a> {
    db: &'a Database,
    embedder: Option<&'a Embedder>,
}

impl<'a> Searcher<'a> {
    pub fn new(db: &'a Database, embedder: Option<&'a Embedder>) -> Self {
        Self { db, embedder }
    }

    /// Search by symbol name with LRU caching
    pub fn search_symbol(&self, name: &str, chunk_type: Option<&str>) -> Result<Vec<SearchResult>> {
        let cache_key = if let Some(ct) = chunk_type {
            format!("sym:{}:{}", name, ct)
        } else {
            format!("sym:{}", name)
        };

        // Check cache
        if let Ok(mut cache) = CACHE.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                return Ok(cached.clone());
            }
        }

        let results = self.search_symbol_uncached(name, chunk_type)?;

        // Store in cache
        if let Ok(mut cache) = CACHE.lock() {
            cache.put(cache_key, results.clone());
        }

        Ok(results)
    }

    /// Search symbol without cache lookup
    fn search_symbol_uncached(&self, name: &str, chunk_type: Option<&str>) -> Result<Vec<SearchResult>> {
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

    /// Semantic search with LRU caching
    pub fn semantic_search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let cache_key = format!("sem:{}:{}", query, limit);

        if let Ok(mut cache) = CACHE.lock() {
            if let Some(cached) = cache.get(&cache_key) {
                return Ok(cached.clone());
            }
        }

        let results = self.semantic_search_uncached(query, limit)?;

        if let Ok(mut cache) = CACHE.lock() {
            cache.put(cache_key, results.clone());
        }

        Ok(results)
    }

    /// Semantic search without cache lookup (brute-force cosine similarity)
    fn semantic_search_uncached(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
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
