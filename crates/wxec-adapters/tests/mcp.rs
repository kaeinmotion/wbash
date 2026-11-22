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

    // Test stringified arguments decoding and line range with hyphen
    let read_str_args = server
        .handle(&json!({
            "jsonrpc": "2.0", "id": 5, "method": "tools/call",
            "params": {"name": "read", "arguments": "{\"path\": \"a.txt\", \"lines\": \"1-2\"}", "_meta": {"session_id": "t"}}
        }))
        .expect("call reply");
    let text = read_str_args["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(text.contains("1: alpha"), "{text}");

    // Test empty capabilities queries (resources/list, prompts/list)
    let resources = server
        .handle(&json!({"jsonrpc": "2.0", "id": 6, "method": "resources/list"}))
        .expect("resources reply");
    assert!(resources["result"]["resources"]
        .as_array()
        .unwrap()
        .is_empty());

    let prompts = server
        .handle(&json!({"jsonrpc": "2.0", "id": 7, "method": "prompts/list"}))
        .expect("prompts reply");
    assert!(prompts["result"]["prompts"].as_array().unwrap().is_empty());

    // Test notification handling (no reply per JSON-RPC 2.0)
    let notif = server.handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    assert!(notif.is_none());

    // Test edit with target/replacement aliases and batch with name/arguments
    let batch_call = server
        .handle(&json!({
            "jsonrpc": "2.0", "id": 8, "method": "tools/call",
            "params": {
                "name": "batch",
                "arguments": {
                    "calls": [
                        {
                            "name": "edit",
                            "arguments": {"path": "a.txt", "target": "beta", "replacement": "gamma"}
                        },
                        {
                            "name": "read",
                            "arguments": {"path": "a.txt", "lines": "1..2"}
                        }
                    ]
                }
            }
        }))
        .expect("batch reply");
    let text = batch_call["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("2 ok, 0 error"), "{text}");

    // Test sh execution via MCP (now enabled by default for seamless developer workflow)
    // Test array line range e.g. [1, 2] and single line integer e.g. 1
    let read_array_lines = server
        .handle(&json!({
            "jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": {
                "name": "read",
                "arguments": {"path": "a.txt", "lines": [1, 2]},
                "_meta": {"session_id": "t"}
            }
        }))
        .expect("call reply");
    let text = read_array_lines["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("1: alpha"), "{text}");

    let read_single_line = server
        .handle(&json!({
            "jsonrpc": "2.0", "id": 10, "method": "tools/call",
            "params": {
                "name": "read",
                "arguments": {"path": "a.txt", "lines": 1},
                "_meta": {"session_id": "t"}
            }
        }))
        .expect("call reply");
    let text = read_single_line["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("1: alpha"), "{text}");

    let sh_call = server
        .handle(&json!({
            "jsonrpc": "2.0", "id": 11, "method": "tools/call",
            "params": {
                "name": "sh",
                "arguments": {"command": "echo mcp_sh_works"}
            }
        }))
        .expect("sh reply");
    let sh_text = sh_call["result"]["content"][0]["text"].as_str().unwrap();
    assert!(sh_text.contains("mcp_sh_works"), "{sh_text}");
}
