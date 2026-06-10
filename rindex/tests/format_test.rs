/// Token savings: compare compact text format vs serde_json::to_string_pretty
#[cfg(test)]
mod tests {
    use rindex::search::{group_by_file, format_compact, format_compact_symbol, SearchResult};

    fn make_results(count: usize) -> Vec<SearchResult> {
        let mut rs = Vec::new();
        let files = ["src/main.rs", "src/lib.rs", "src/api/handlers.rs",
                     "src/db/queries.rs", "src/mcp/mod.rs"];
        let symbols = ["fn parse_config", "struct Config", "trait Handler",
                       "fn handle_request", "async fn search", "fn new",
                       "impl Database", "fn connect", "mod tests", "const MAX"];
        for i in 0..count {
            rs.push(SearchResult {
                file_path: files[i % files.len()].to_string(),
                chunk_type: "function".to_string(),
                name: Some(symbols[i % symbols.len()].to_string()),
                start_line: (i * 20 + 1) as i64,
                end_line: (i * 20 + 15) as i64,
                snippet: format!(
                    "{} {}\n    // documentation\n    let x = {};\n    x + 1\n}}",
                    symbols[i % symbols.len()], i, i + 42
                ),
                score: 0.95 - (i as f64 * 0.01),
            });
        }
        rs
    }

    fn count_tokens(s: &str) -> usize {
        // Rough approximation: 1 token ≈ 4 chars for code
        s.len() / 4
    }

    #[test]
    fn test_compact_vs_json_token_savings() {
        let results_5 = make_results(5);
        let results_10 = make_results(10);

        // 5 results
        let grouped_5 = group_by_file(results_5);
        let json_5 = serde_json::to_string_pretty(&grouped_5).unwrap();
        let compact_5 = format_compact(&grouped_5);
        let compact_sym_5 = format_compact_symbol(&grouped_5);

        // 10 results
        let grouped_10 = group_by_file(results_10);
        let json_10 = serde_json::to_string_pretty(&grouped_10).unwrap();
        let compact_10 = format_compact(&grouped_10);
        let compact_sym_10 = format_compact_symbol(&grouped_10);

        let save_5 = (1.0 - compact_5.len() as f64 / json_5.len() as f64) * 100.0;
        let save_sym_5 = (1.0 - compact_sym_5.len() as f64 / json_5.len() as f64) * 100.0;
        let save_10 = (1.0 - compact_10.len() as f64 / json_10.len() as f64) * 100.0;
        let save_sym_10 = (1.0 - compact_sym_10.len() as f64 / json_10.len() as f64) * 100.0;

        println!("\n=== TOKEN SAVINGS: format_compact vs JSON ===");
        println!("--- 5 results ({:?} files) ---", grouped_5.len());
        println!("  JSON pretty:       {:>5} chars / ~{:>4} tokens", json_5.len(), count_tokens(&json_5));
        println!("  format_compact:    {:>5} chars / ~{:>4} tokens  (saves {:.0}%)", compact_5.len(), count_tokens(&compact_5), save_5);
        println!("  compact_symbol:    {:>5} chars / ~{:>4} tokens  (saves {:.0}%)", compact_sym_5.len(), count_tokens(&compact_sym_5), save_sym_5);
        println!("--- 10 results ({:?} files) ---", grouped_10.len());
        println!("  JSON pretty:       {:>5} chars / ~{:>4} tokens", json_10.len(), count_tokens(&json_10));
        println!("  format_compact:    {:>5} chars / ~{:>4} tokens  (saves {:.0}%)", compact_10.len(), count_tokens(&compact_10), save_10);
        println!("  compact_symbol:    {:>5} chars / ~{:>4} tokens  (saves {:.0}%)", compact_sym_10.len(), count_tokens(&compact_sym_10), save_sym_10);

        // Assertions: compact should be at least 30% smaller
        assert!(compact_5.len() < json_5.len() * 7 / 10,
            "format_compact should save >=30% vs JSON (5 results): saves {:.0}%", save_5);
        assert!(compact_10.len() < json_10.len() * 7 / 10,
            "format_compact should save >=30% vs JSON (10 results): saves {:.0}%", save_10);
        assert!(compact_sym_10.len() < json_10.len() * 6 / 10,
            "format_compact_symbol should save >=40% vs JSON (10 results): saves {:.0}%", save_sym_10);
    }

    #[test]
    fn test_compact_format_output() {
        let results = vec![SearchResult {
            file_path: "src/main.rs".to_string(),
            chunk_type: "function".to_string(),
            name: Some("main".to_string()),
            start_line: 1,
            end_line: 10,
            snippet: "fn main() {\n    println!(\"hello\");\n}".to_string(),
            score: 0.95,
        }];
        let grouped = group_by_file(results);
        let compact = format_compact(&grouped);
        assert!(compact.contains("# src/main.rs"), "Should show file path header");
        assert!(compact.contains("L1"), "Should show line number");
        assert!(compact.contains("main"), "Should show symbol name");
        assert!(!compact.contains("\"file_path\""), "No JSON field names in output");
    }
}
