#[cfg(test)]
mod tests {
    use rindex::db::Database;
    use rindex::db::queries::*;

    #[test]
    fn test_upsert_file() {
        let db = Database::open_temp().unwrap();
        upsert_file(&db, "src/main.rs", "abc123", 1024, 1000, "rust", 2000).unwrap();
        let file = get_file(&db, "src/main.rs").unwrap().unwrap();
        assert_eq!(file.path, "src/main.rs");
        assert_eq!(file.hash, "abc123");
        assert_eq!(file.language, Some("rust".to_string()));
    }

    #[test]
    fn test_delete_file_cascades_to_chunks() {
        let db = Database::open_temp().unwrap();
        upsert_file(&db, "src/lib.rs", "def456", 512, 1000, "rust", 2000).unwrap();
        insert_chunk(&db, "src/lib.rs", "function", Some("hello"), None, 1, 10, "fn hello() {}").unwrap();
        delete_file(&db, "src/lib.rs").unwrap();
        let chunks = get_chunks_for_file(&db, "src/lib.rs").unwrap();
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_get_or_create_project() {
        let db = Database::open_temp().unwrap();
        let proj = get_or_create_project(&db, "/test/project").unwrap();
        assert_eq!(proj.root_path, "/test/project");
    }

    #[test]
    fn test_update_project_stats() {
        let db = Database::open_temp().unwrap();
        update_project_stats(&db, "/test/proj", 10, 42).unwrap();
        let proj = get_or_create_project(&db, "/test/proj").unwrap();
        assert_eq!(proj.file_count, 10);
        assert_eq!(proj.chunk_count, 42);
    }
}
