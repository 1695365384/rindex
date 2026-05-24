use crate::db::Database;
use crate::embedding::Embedder;
use anyhow::Result;
use lru::LruCache;
use std::fmt::Write as FmtWrite;
use std::num::NonZeroUsize;

/// Default number of search results to cache
const CACHE_SIZE: usize = 64;

/// Maximum snippet lines before truncation
const MAX_SNIPPET_LINES: usize = 20;

/// Lines to show from head when snippet is truncated
const SNIPPET_HEAD_LINES: usize = 10;

/// Lines to show from tail when snippet is truncated
const SNIPPET_TAIL_LINES: usize = 5;

/// Trim a multi-line snippet to avoid blowing up Claude Code's context.
/// If the snippet has more than MAX_SNIPPET_LINES lines, show the first
/// SNIPPET_HEAD_LINES and last SNIPPET_TAIL_LINES with a truncation notice.
fn trim_snippet(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= MAX_SNIPPET_LINES {
        return content.to_string();
    }
    let omitted = lines.len() - SNIPPET_HEAD_LINES - SNIPPET_TAIL_LINES;
    let head = lines[..SNIPPET_HEAD_LINES].join("\n");
    let tail = lines[lines.len() - SNIPPET_TAIL_LINES..].join("\n");
    format!(
        "{head}\n... ({omitted} lines omitted) ...\n{tail}"
    )
}

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

/// Format grouped results as compact text, avoiding JSON overhead.
/// Saves ~35% tokens vs serde_json::to_string_pretty.
///
/// ```text
/// # src/main.rs [0.92]
///   L12 | function parse_config | fn parse_config(path: &str) ...
///   L45 | trait Handler | async fn handle(&self, req: Request) ...
/// ```
pub fn format_compact(results: &[FileGroupedResult]) -> String {
    let mut out = String::with_capacity(results.len() * 120);
    for file in results {
        let _ = writeln!(out, "# {} [{:.2}]", file.file_path, file.total_score);
        for m in &file.matches {
            let first_line = m.snippet.lines().next().unwrap_or("");
            match &m.name {
                Some(name) => {
                    let _ = writeln!(out, "  L{} | {} {} | {}", m.line, m.symbol_type, name, first_line);
                }
                None => {
                    let _ = writeln!(out, "  L{} | {} | {}", m.line, m.symbol_type, first_line);
                }
            }
        }
    }
    out
}

/// Compact format without scores or chunk types — for exact symbol search
/// where scores are predictable (1.0-3.0) and type is known to the caller.
/// Saves ~50% tokens vs JSON.
///
/// ```text
/// # src/main.rs
///   L12 | parse_config | fn parse_config(path: &str) ...
///   L45 | Handler | async fn handle(&self, req: Request) ...
/// ```
pub fn format_compact_symbol(results: &[FileGroupedResult]) -> String {
    let mut out = String::with_capacity(results.len() * 100);
    for file in results {
        let _ = writeln!(out, "# {}", file.file_path);
        for m in &file.matches {
            let first_line = m.snippet.lines().next().unwrap_or("");
            match &m.name {
                Some(name) => {
                    let _ = writeln!(out, "  L{} | {} | {}", m.line, name, first_line);
                }
                None => {
                    let _ = writeln!(out, "  L{} | {}", m.line, first_line);
                }
            }
        }
    }
    out
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

        Ok(rows.into_iter().map(|mut r| {
            r.snippet = trim_snippet(&r.snippet);
            r
        }).collect())
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

    /// Find code semantically related to a given symbol or file location.
    /// Uses embedding similarity to discover conceptually similar code elsewhere in the project.
    /// If `name` is provided, finds the best-matching chunk by symbol name.
    /// If `file_path` + `line` is provided, finds the chunk at that line.
    pub fn find_related(
        &self,
        name: Option<&str>,
        file_path: Option<&str>,
        line: Option<i64>,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let conn = self.db.conn()?;

        // Step 1: Locate the source chunk and its embedding
        let source = if let Some(n) = name {
            // Find by symbol name (best match)
            let mut stmt = conn.prepare(
                "SELECT id, file_path, chunk_type, name, start_line, end_line, content, embedding
                 FROM chunks
                 WHERE (name = ?1 OR name LIKE ?2)
                   AND embedding IS NOT NULL
                 ORDER BY
                   CASE WHEN name = ?1 THEN 0 ELSE 1 END
                 LIMIT 1"
            )?;
            let like = format!("%{}%", n);
            let result: Vec<_> = stmt.query_map(
                rusqlite::params![n, &like],
                |row| {
                    let blob: Vec<u8> = row.get(7)?;
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, blob))
                },
            )?.filter_map(|r| r.ok()).collect();
            drop(stmt);
            result.into_iter().next()
        } else if let Some(fp) = file_path {
            // Find by file path + line number
            let line_val = line.unwrap_or(0);
            let mut stmt = conn.prepare(
                "SELECT id, file_path, chunk_type, name, start_line, end_line, content, embedding
                 FROM chunks
                 WHERE file_path = ?1 AND start_line <= ?2 AND end_line >= ?2
                   AND embedding IS NOT NULL
                 LIMIT 1"
            )?;
            let result: Vec<_> = stmt.query_map(
                rusqlite::params![fp, line_val],
                |row| {
                    let blob: Vec<u8> = row.get(7)?;
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, blob))
                },
            )?.filter_map(|r| r.ok()).collect();
            drop(stmt);
            result.into_iter().next()
        } else {
            return Ok(Vec::new());
        };

        let (source_id, source_path, source_emb_blob) = match source {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };

        // Deserialize source embedding
        let source_vec: Vec<f32> = source_emb_blob.chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        // Also get the source chunk_type for type-match bonus
        let source_type: String = {
            let mut stmt = conn.prepare(
                "SELECT chunk_type FROM chunks WHERE id = ?1"
            )?;
            stmt.query_row(rusqlite::params![source_id], |row| row.get(0))?
        };

        let source_path_clone = source_path.clone();

        // Step 2: Load candidate chunks and compute similarity
        let candidate_limit = std::cmp::max(limit as i64 * 10, 200);
        let mut stmt = conn.prepare(
            "SELECT c.id, c.file_path, c.chunk_type, c.name, c.start_line, c.end_line,
                    c.content, c.embedding
             FROM chunks c
             WHERE c.id != ?1
               AND c.embedding IS NOT NULL
             ORDER BY c.id
             LIMIT ?2"
        )?;

        let mut scored: Vec<(SearchResult, f64)> = stmt.query_map(
            rusqlite::params![source_id, candidate_limit],
            |row| {
                let blob: Vec<u8> = row.get(7)?;
                Ok((SearchResult {
                    file_path: row.get(1)?,
                    chunk_type: row.get(2)?,
                    name: row.get(3)?,
                    start_line: row.get(4)?,
                    end_line: row.get(5)?,
                    snippet: row.get(6)?,
                    score: 0.0,
                }, blob))
            },
        )?
        .filter_map(|r| r.ok())
        .map(|(mut result, blob)| {
            let vec: Vec<f32> = blob.chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();

            // Embedding cosine similarity (0.0 - 1.0)
            let emb_score: f64 = source_vec.iter()
                .zip(vec.iter())
                .map(|(a, b)| (*a as f64) * (*b as f64))
                .sum::<f64>();

            // Type-match bonus: same chunk_type → +0.15
            let type_bonus = if result.chunk_type == source_type { 0.15 } else { 0.0 };

            // Same-file penalty: -0.3 to avoid trivial same-file results
            let file_penalty = if result.file_path == source_path_clone { 0.3 } else { 0.0 };

            let score = emb_score + type_bonus - file_penalty;
            result.score = score;
            (result, score)
        })
        .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored.into_iter().take(limit).map(|(r, _)| {
            let mut r = r;
            r.snippet = trim_snippet(&r.snippet);
            r
        }).collect())
    }

    /// Two-stage hybrid semantic search:
    /// Stage 1: FTS5 full-text pre-filter to generate candidates — fast, keyword-aware
    /// Stage 2: Compute embedding similarity ONLY on candidates for re-ranking
    ///
    /// This avoids loading ALL chunk embeddings into memory, which is the bottleneck
    /// for large projects (100k+ chunks).
    fn semantic_search_uncached(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        let query_vec = match self.embedder {
            Some(emb) => Some(emb.embed(query)?),
            None => return self.search_symbol(query, None),
        };

        let conn = self.db.conn()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
        let recent_threshold = now - 86400;

        // Stage 1: FTS5 pre-filter — use AND semantics for precision.
        // OR was too loose: common English words in queries matched noise files
        // (e.g. "store", "state" in DEPLOY.md). AND requires ALL meaningful
        // tokens to be present in the chunk, filtering out irrelevant documents.
        let candidate_limit = std::cmp::max(limit as i64 * 5, 50);

        // Build FTS5 query: split by whitespace, filter short tokens, join with AND
        let fts_query: String = query
            .split_whitespace()
            .filter(|t| t.len() >= 2)
            .map(|t| {
                if t.contains(|c: char| c == '"' || c == '(' || c == ')' || c == '*' || c == '^') {
                    format!("\"{}\"", t.replace('"', "\"\""))
                } else {
                    t.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" AND ");

        if fts_query.is_empty() {
            return Ok(Vec::new());
        }

        // Fallback: if AND is too strict (zero candidates), retry with OR
        let fts_query_fallback: String = fts_query.replace(" AND ", " OR ");

        // Execute FTS5 query: try AND first, fall back to OR if too strict
        let candidate_rows: Vec<(SearchResult, Vec<u8>, i64)> = {
            let mut stmt = conn.prepare(
                "SELECT c.file_path, c.chunk_type, c.name, c.start_line, c.end_line,
                        c.content, c.embedding, f.mtime
                 FROM chunks c JOIN files f ON c.file_path = f.path
                 WHERE c.embedding IS NOT NULL
                   AND c.id IN (SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ?1)
                 ORDER BY
                   CASE
                     WHEN LOWER(c.name) = LOWER(?2) THEN 3.0
                     WHEN LOWER(c.file_path) LIKE ?2 THEN 2.0
                     WHEN LOWER(c.name) LIKE ?2 THEN 1.5
                     ELSE 1.0
                   END DESC
                 LIMIT ?3"
            )?;

            let rows: Vec<_> = stmt.query_map(
                rusqlite::params![&fts_query, query, candidate_limit],
                |row| {
                    let blob: Vec<u8> = row.get(6)?;
                    let mtime: i64 = row.get(7)?;
                    Ok((SearchResult {
                        file_path: row.get(0)?,
                        chunk_type: row.get(1)?,
                        name: row.get(2)?,
                        start_line: row.get(3)?,
                        end_line: row.get(4)?,
                        snippet: row.get(5)?,
                        score: 0.0,
                    }, blob, mtime))
                },
            )?
            .filter_map(|r| r.ok())
            .collect();

            // Fallback: if AND returns nothing, retry with OR
            if rows.is_empty() {
                tracing::debug!(
                    "FTS5 AND query returned 0 results, falling back to OR: {}",
                    fts_query_fallback
                );
                stmt.query_map(
                    rusqlite::params![&fts_query_fallback, query, candidate_limit],
                    |row| {
                        let blob: Vec<u8> = row.get(6)?;
                        let mtime: i64 = row.get(7)?;
                        Ok((SearchResult {
                            file_path: row.get(0)?,
                            chunk_type: row.get(1)?,
                            name: row.get(2)?,
                            start_line: row.get(3)?,
                            end_line: row.get(4)?,
                            snippet: row.get(5)?,
                            score: 0.0,
                        }, blob, mtime))
                    },
                )?
                .filter_map(|r| r.ok())
                .collect()
            } else {
                rows
            }
        };

        // Stage 2: Re-rank candidates by hybrid score
        // Embedding is the primary signal (50%) — the distilled C2LLM model
        // produces code-aware embeddings that understand identifiers and semantics.
        // Text relevance (40%) provides exact-match precision.
        let mut scored: Vec<(SearchResult, f64)> = candidate_rows
            .into_iter()
            .map(|(mut result, blob, mtime)| {
                let vec: Vec<f32> = blob.chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();

                // 1. Embedding similarity (cosine dot product, 0.0 - 1.0)
                let emb_score: f64 = query_vec.as_ref().map_or(0.0, |qv| {
                    qv.iter()
                        .zip(vec.iter())
                        .map(|(a, b)| (*a as f64) * (*b as f64))
                        .sum::<f64>()
                });

                // 2. Text relevance (0.0 - 3.0, normalized to 0-1)
                let file_path_lower = result.file_path.to_lowercase();
                let query_lower = query.to_lowercase();
                let name = result.name.as_deref().unwrap_or("");
                let text_score = if name.eq_ignore_ascii_case(query) {
                    3.0
                } else if file_path_lower.contains(&query_lower) {
                    2.0
                } else if name.to_lowercase().contains(&query_lower) {
                    1.5
                } else {
                    1.0
                } / 3.0;

                // 3. Freshness (0.0 - 0.5, normalized)
                let fresh_score = if mtime > recent_threshold { 0.5 } else { 0.0 } / 3.0;

                // Hybrid: embedding (50%), text (40%), freshness (10%)
                let score = emb_score * 0.50 + text_score * 0.40 + fresh_score * 0.10;
                result.score = score;
                (result, score)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored.into_iter().take(limit).map(|(r, _)| {
            let mut r = r;
            r.snippet = trim_snippet(&r.snippet);
            r
        }).collect())
    }
}
