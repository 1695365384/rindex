#[cfg(test)]
mod tests {
    use rindex::mcp::{McpRequest, parse_request, get_tool_definitions};

    #[test]
    fn test_parse_initialize_request() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;
        let req: McpRequest = parse_request(json).unwrap();
        assert_eq!(req.id, Some(serde_json::Value::Number(1.into())));
        assert_eq!(req.method, "initialize");
    }

    #[test]
    fn test_parse_request_without_id() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#;
        let req: McpRequest = parse_request(json).unwrap();
        assert!(req.id.is_none());
    }

    #[test]
    fn test_tool_definitions_exist() {
        let tools = get_tool_definitions();
        assert!(!tools.is_empty());
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"search"));
        assert!(names.contains(&"search_symbol"));
        assert!(names.contains(&"project_status"));
    }
}
