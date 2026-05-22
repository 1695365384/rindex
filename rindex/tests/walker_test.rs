#[cfg(test)]
mod tests {
    use rindex::indexer::walker::FileWalker;
    use rindex::ignore::IgnoreEngine;
    use std::path::Path;

    #[test]
    fn test_walk_project_root() {
        let engine = IgnoreEngine::default();
        let walker = FileWalker::new(&engine);
        let files = walker.walk(Path::new(".")).unwrap();
        assert!(files.iter().any(|f| f.path.ends_with("Cargo.toml")));
    }

    #[test]
    fn test_skip_git_directory() {
        let engine = IgnoreEngine::default();
        let walker = FileWalker::new(&engine);
        let files = walker.walk(Path::new(".")).unwrap();
        assert!(!files.iter().any(|f| f.relative_path.contains(".git")));
    }

    #[test]
    fn test_skip_node_modules() {
        let engine = IgnoreEngine::default();
        let walker = FileWalker::new(&engine);
        let files = walker.walk(Path::new(".")).unwrap();
        assert!(!files.iter().any(|f| f.relative_path.contains("node_modules")));
    }
}
