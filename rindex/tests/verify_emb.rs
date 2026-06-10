#[cfg(test)]
mod tests {
    use rindex::db::Database;
    use std::path::Path;

    #[test]
    fn test_embeddings_exist() {
        let db = Database::open(Path::new("C:/Users/bundy/AppData/Roaming/rindex/rindex.db")).unwrap();
        let conn = db.conn().unwrap();
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap();
        let with_emb: i64 = conn.query_row("SELECT COUNT(*) FROM chunks WHERE embedding IS NOT NULL", [], |r| r.get(0)).unwrap();
        let emb_size: i64 = conn.query_row(
            "SELECT LENGTH(embedding) FROM chunks WHERE embedding IS NOT NULL LIMIT 1",
            [], |r| r.get(0),
        ).unwrap();
        let pct = (with_emb as f64 / total as f64) * 100.0;
        println!("Total chunks: {}", total);
        println!("With embedding: {} ({:.0}%)", with_emb, pct);
        println!("Embedding dims: {} (384 f32s = 1536 bytes)", emb_size / 4);
        assert!(with_emb > 0, "Should have embeddings after backfill");
        assert_eq!(emb_size, 1536, "Embedding should be 384 f32s = 1536 bytes");
    }
}
