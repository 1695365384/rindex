#[cfg(test)]
mod tests {
    use rindex::mcp::get_tool_definitions;

    #[test]
    fn test_tool_definitions_exist() {
        let tools = get_tool_definitions();
        assert!(!tools.is_empty());
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"search"));
        assert!(names.contains(&"search_symbol"));
        assert!(names.contains(&"project_status"));
    }

    #[test]
    fn test_backfill_tool_exists() {
        let tools = get_tool_definitions();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"backfill"));
    }

    #[test]
    fn test_reindex_tool_exists() {
        let tools = get_tool_definitions();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"reindex"));
        assert!(names.contains(&"verify"));
    }
}
