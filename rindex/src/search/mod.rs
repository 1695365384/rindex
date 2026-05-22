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

/// Search results grouped by file — more useful for Claude Code consumption
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileGroupedResult {
    pub file_path: String,
    pub matches: Vec<SymbolMatch>,
    pub total_score: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SymbolMatch {
    pub symbol_type: String,
    pub name: Option<String>,
    pub line: i64,
    pub snippet: String,
    pub score: f64,
}

/// Group a flat list of search results by file path
pub fn group_by_file(results: Vec<SearchResult>) -> Vec<FileGroupedResult> {
    use std::collections::HashMap;
    let mut grouped: HashMap<String, FileGroupedResult> = HashMap::new();

    for r in results {
        let entry = grouped.entry(r.file_path.clone()).or_insert_with(|| FileGroupedResult {
            file_path: r.file_path.clone(),
            matches: Vec::new(),
            total_score: 0.0,
        });
        entry.matches.push(SymbolMatch {
            symbol_type: r.chunk_type,
            name: r.name,
            line: r.start_line,
            snippet: r.snippet,
            score: r.score,
        });
        entry.total_score += r.score;
    }

    // Sort by total_score descending
    let mut result: Vec<FileGroupedResult> = grouped.into_values().collect();
    result.sort_by(|a, b| b.total_score.partial_cmp(&a.total_score).unwrap_or(std::cmp::Ordering::Equal));

    // Sort matches within each file by line number
    for file in &mut result {
        file.matches.sort_by(|a, b| a.line.cmp(&b.line));
    }

    result
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

    /// Search symbol without cache lookup (includes file path and content matching)
    fn search_symbol_uncached(&self, name: &str, chunk_type: Option<&str>) -> Result<Vec<SearchResult>> {
        let conn = self.db.conn()?;
        let like_pattern = format!("%{}%", name);

        // Search across name, file_path, and content; boost recent files
        let sql = if chunk_type.is_some() {
            "SELECT c.file_path, c.chunk_type, c.name, c.start_line, c.end_line, c.content,
                    CASE
                      WHEN c.name = ?1 THEN 3.0           -- exact symbol match
                      WHEN c.file_path LIKE ?2 THEN 2.0    -- file path match
                      WHEN c.name LIKE ?2 THEN 1.5         -- partial symbol match
                      ELSE 1.0                              -- content match
                    END +
                    CASE WHEN f.mtime > ?4 THEN 0.5 ELSE 0 END  -- freshness boost
                    AS score
             FROM chunks c JOIN files f ON c.file_path = f.path
             WHERE (c.name LIKE ?2 OR c.file_path LIKE ?2 OR c.content LIKE ?2)
               AND c.chunk_type = ?3
             ORDER BY score DESC
             LIMIT ?5"
        } else {
            "SELECT c.file_path, c.chunk_type, c.name, c.start_line, c.end_line, c.content,
                    CASE
                      WHEN c.name = ?1 THEN 3.0
                      WHEN c.file_path LIKE ?2 THEN 2.0
                      WHEN c.name LIKE ?2 THEN 1.5
                      ELSE 1.0
                    END +
                    CASE WHEN f.mtime > ?3 THEN 0.5 ELSE 0 END
                    AS score
             FROM chunks c JOIN files f ON c.file_path = f.path
             WHERE (c.name LIKE ?2 OR c.file_path LIKE ?2 OR c.content LIKE ?2)
             ORDER BY score DESC
             LIMIT ?4"
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let recent_threshold = now - 86400; // last 24 hours

        let mut stmt = conn.prepare(sql)?;

        let limit: i64 = 20;
        let rows: Vec<SearchResult> = if let Some(ctype) = chunk_type {
            stmt.query_map(
                rusqlite::params![name, like_pattern, ctype, recent_threshold, limit],
                |row| Ok(SearchResult {
                    file_path: row.get(0)?, chunk_type: row.get(1)?,
                    name: row.get(2)?, start_line: row.get(3)?,
                    end_line: row.get(4)?, snippet: row.get(5)?,
                    score: row.get(6)?,
                })
            )?.filter_map(|r| r.ok()).collect()
        } else {
            stmt.query_map(
                rusqlite::params![name, like_pattern, recent_threshold, limit], // ?1, ?2, ?3, ?4
                |row| Ok(SearchResult {
                    file_path: row.get(0)?, chunk_type: row.get(1)?,
                    name: row.get(2)?, start_line: row.get(3)?,
                    end_line: row.get(4)?, snippet: row.get(5)?,
                    score: row.get(6)?,
                })
            )?.filter_map(|r| r.ok()).collect()
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

    /// Hybrid semantic search: combines embedding similarity with text-based relevance
    fn semantic_search_uncached(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let query_vec = match self.embedder {
            Some(emb) => emb.embed(query)?,
            None => return self.search_symbol(query, None),
        };

        let conn = self.db.conn()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let recent_threshold = now - 86400;
        // Join with files table for mtime freshness, compute text relevance inline
        let mut stmt = conn.prepare(
            "SELECT c.id, c.file_path, c.chunk_type, c.name, c.start_line, c.end_line,
                    c.content, c.embedding, f.mtime
             FROM chunks c JOIN files f ON c.file_path = f.path
             WHERE c.embedding IS NOT NULL"
        )?;

        // Compute hybrid score: 70% embedding similarity + 30% text relevance
        let mut scored: Vec<(SearchResult, f64)> = stmt.query_map([], |row| {
            let blob: Vec<u8> = row.get(7)?;
            let vec: Vec<f32> = blob.chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            let mtime: i64 = row.get(8)?;
            Ok((SearchResult {
                file_path: row.get(1)?,
                chunk_type: row.get(2)?,
                name: row.get(3)?,
                start_line: row.get(4)?,
                end_line: row.get(5)?,
                snippet: row.get(6)?,
                score: 0.0,
            }, vec, mtime))
        })?
        .filter_map(|r| r.ok())
        .map(|(mut result, vec, mtime)| {
            // 1. Embedding similarity (0.0 - 1.0)
            let emb_score: f64 = query_vec.iter()
                .zip(vec.iter())
                .map(|(a, b)| (*a as f64) * (*b as f64))
                .sum::<f64>() as f64;

            // 2. Text relevance (0.0 - 3.0, normalized to 0-1)
            let file_path_lower = result.file_path.to_lowercase();
            let query_lower = query.to_lowercase();
            let name = result.name.as_deref().unwrap_or("");
            let text_score = if name == query { 3.0 }
                else if file_path_lower.contains(&query_lower) { 2.0 }
                else if name.to_lowercase().contains(&query_lower) { 1.5 }
                else { 1.0 } / 3.0; // normalize

            // 3. Freshness (0.0 - 0.5, normalized)
            let fresh_score = if mtime > recent_threshold { 0.5 } else { 0.0 } / 3.0;

            // Hybrid: 60% embedding, 30% text, 10% freshness
            let score = emb_score * 0.6 + text_score * 0.3 + fresh_score * 0.1;
            result.score = score;
            (result, score)
        })
        .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored.into_iter().take(limit).map(|(r, _)| r).collect())
    }
}
