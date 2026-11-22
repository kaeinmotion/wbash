use std::path::PathBuf;

use serde_json::json;
use wxec_adapters::McpServer;

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn mcp_handshake_list_and_call() {
    let root: PathBuf = std::env::temp_dir().join(format!("wxec-mcp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.txt"), "alpha\nbeta\n").unwrap();
    let cache = root.with_extension("cache");
    let config_home = root.with_extension("config");
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);

    let mut server = McpServer::open(&root, None, true, None).unwrap();

    let init = server
        .handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}}))
        .expect("initialize reply");
    assert_eq!(
        init["result"]["protocolVersion"],
        wxec_adapters::PROTOCOL_VERSION
    );
    assert!(init["result"]["capabilities"]["tools"].is_object());

    let list = server
        .handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}))
        .expect("tools/list reply");
    let tools = list["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for want in [
        "tree", "read", "search", "edit", "run", "check", "git", "batch", "expand",
    ] {
        assert!(names.contains(&want), "missing {want} in {names:?}");
    }
    let schema_chars = serde_json::to_string(&tools).unwrap().len();
    assert!(
        schema_chars / 4 <= 1500,
        "schema budget {} est tokens",
        schema_chars / 4
    );

    let read = server
        .handle(&json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "read", "arguments": {"path": "a.txt"}, "_meta": {"session_id": "t"}}
        }))
        .expect("call reply");
    let text = read["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("1: alpha"), "{text}");

    let err = server
        .handle(&json!({"jsonrpc": "2.0", "id": 4, "method": "nope"}))
        .expect("error reply");
    assert_eq!(err["error"]["code"], -32601);
}
