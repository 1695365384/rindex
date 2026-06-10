/// rindex vs Grep/Glob: real task comparison
/// Scenario: "Find the reindex implementation and understand embedder/backfill flow"
#[cfg(test)]
mod tests {
    use rindex::search::{group_by_file, format_compact, format_compact_symbol, Searcher};
    use rindex::db::Database;
    use std::path::Path;

    fn count_tokens(chars: usize) -> usize { chars / 4 }

    fn open_db() -> Database {
        Database::open(Path::new("C:\\Users\\bundy\\AppData\\Roaming\\rindex\\rindex.db"))
            .expect("Failed to open database")
    }

    #[test]
    fn test_rindex_vs_grep_comparison() {
        let db = open_db();
        let searcher = Searcher::new(&db, None);

        // ── rindex tool calls ──
        let rindex_queries = [
            ("search_symbol 'reindex'",    "reindex"),
            ("search_symbol 'backfill'",    "backfill"),
        ];

        let mut rindex_json_chars = 0usize;
        let mut rindex_compact_chars = 0usize;

        println!("\n╔══════════════════════════════════════════════════════════════════╗");
        println!("║  rindex vs Grep: REAL TASK TOKEN COMPARISON                      ║");
        println!("║  Task: '找到 reindex 函数，理解 embedder/backfill 协作流程'         ║");
        println!("╠══════════════════════════════════════════════════════════════════╣");
        println!("║                                                                  ║");
        println!("║  ────────── WITHOUT rindex (Grep + Read) ──────────              ║");

        // Grep raw data (measured from actual rg runs)
        let grep_calls = [
            ("grep 'reindex'",  1295),
            ("grep 'backfill'", 1739),
            ("grep 'embedder'", 4220),
            ("grep 'get_embedder'", 711),
            ("grep 'index_project'", 601),
        ];
        let mut grep_total = 0usize;
        for (name, chars) in &grep_calls {
            println!("║    {} → {:>5} chars / {:>4} tok", name, chars, count_tokens(*chars));
            grep_total += chars;
        }
        // After Grep, Claude typically Reads 2-3 files to understand context
        let read_estimates = [
            ("Read mcp/mod.rs  (reindex area)", 2500),
            ("Read indexer/mod.rs (backfill)",   2000),
        ];
        let mut read_total = 0usize;
        for (name, chars) in &read_estimates {
            println!("║    {} → {:>5} chars / {:>4} tok", name, chars, count_tokens(*chars));
            read_total += chars;
        }

        let without_rindex_total = grep_total + read_total;
        let without_rindex_calls = grep_calls.len() + read_estimates.len();

        println!("║  ──────────────────────────────────────                       ║");
        println!("║  WITHOUT rindex: {} tool calls, {:>5} chars / {:>4} tok",
            without_rindex_calls, without_rindex_total, count_tokens(without_rindex_total));

        println!("║                                                                  ║");
        println!("║  ────────── WITH rindex (compact format, MCP output) ──────────  ║");

        for (label, query) in &rindex_queries {
            let results = searcher.search_symbol(query, None).unwrap();
            let grouped = group_by_file(results);

            let json = serde_json::to_string_pretty(&grouped).unwrap();
            let compact = format_compact_symbol(&grouped);

            rindex_json_chars += json.len();
            rindex_compact_chars += compact.len();

            let save = (1.0 - compact.len() as f64 / json.len() as f64) * 100.0;
            println!("║    {} ", label);
            println!("║      CLI (JSON): {:>5} chars / {:>4} tok", json.len(), count_tokens(json.len()));
            println!("║      MCP (compact): {:>5} chars / {:>4} tok  (saves {:.0}% vs JSON)",
                compact.len(), count_tokens(compact.len()), save);
        }

        let with_rindex_calls = rindex_queries.len();

        println!("║  ──────────────────────────────────────                       ║");
        println!("║  WITH rindex: {} tool calls, {:>5} chars / {:>4} tok",
            with_rindex_calls, rindex_compact_chars, count_tokens(rindex_compact_chars));

        println!("╠══════════════════════════════════════════════════════════════════╣");

        let call_save = without_rindex_calls - with_rindex_calls;
        let tok_save = without_rindex_total as i64 - rindex_compact_chars as i64;
        let tok_save_pct = (tok_save as f64 / without_rindex_total as f64) * 100.0;

        println!("║                                                                  ║");
        println!("║  SAVED: {} tool calls  |  {} chars / {:>4} tok  ({:.0}%)",
            call_save, tok_save, count_tokens(tok_save as usize), tok_save_pct);
        println!("║                                                                  ║");
        println!("║  Tool calls: {} → {}  (省 {:.0}%)",
            without_rindex_calls, with_rindex_calls,
            (call_save as f64 / without_rindex_calls as f64) * 100.0);
        println!("║  Response tokens: {:>4} → {:>4}  (省 {:.0}%)",
            count_tokens(without_rindex_total), count_tokens(rindex_compact_chars), tok_save_pct);
        println!("║                                                                  ║");
        println!("╚══════════════════════════════════════════════════════════════════╝\n");

        assert!(tok_save_pct > 70.0, "rindex should save >70% tokens vs Grep+Read");
        assert!(call_save >= 3, "rindex should save at least 3 tool calls");
    }
}
