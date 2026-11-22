use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use wxec_core::config;
use wxec_core::dispatcher::Dispatcher;
use wxec_core::error::{Result, WErr};
use wxec_core::types::Tier;
use wxec_parsers::ParserRegistry;

pub const SERVER_NAME: &str = "wxec";
pub const PROTOCOL_VERSION: &str = "2024-11-05";

pub struct McpServer {
    dispatcher: Dispatcher,
}

impl McpServer {
    pub fn open(
        root: &Path,
        config_path: Option<&Path>,
        no_project_config: bool,
        tier: Option<Tier>,
    ) -> Result<Self> {
        let (mut cfg, warnings) = config::load(root, config_path, no_project_config)?;
        for w in warnings {
            eprintln!("# {w}");
        }
        let t = tier.unwrap_or(Tier::GitWrite);
        cfg.session.default_tier = t.as_str().to_string();
        let registry = Arc::new(wxec_tools::registry());
        let parsers = Arc::new(ParserRegistry::for_root(root, no_project_config));
        let dispatcher = Dispatcher::new(cfg, root, registry, parsers)?;
        Ok(Self { dispatcher })
    }

    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        // JSON-RPC 2.0: Requests without an ID are notifications; servers MUST NOT reply.
        let id = msg.get("id")?.clone();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        let response = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": {"listChanged": false},
                    "resources": {"subscribe": false, "listChanged": false},
                    "prompts": {"listChanged": false}
                },
                "serverInfo": {"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Use wxec tools for workspace operations. Results are compact to reduce token use; use expand with a handle to retrieve full output when needed."
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": self.dispatcher.registry().mcp_tools()})),
            "tools/call" => self.call_tool(&params),
            "resources/list" => Ok(json!({"resources": []})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            "shutdown" => Ok(Value::Null),
            other => Err((-32601, format!("method not found: {other}"))),
        };
        Some(match response {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
            }
        })
    }

    fn call_tool(&mut self, params: &Value) -> std::result::Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((-32602, "missing tool name".to_string()))?
            .to_string();
        let mut args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if let Some(s) = args.as_str() {
            if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                if parsed.is_object() {
                    args = parsed;
                }
            }
        }
        let session = params
            .get("_meta")
            .and_then(|m| m.get("session_id"))
            .or_else(|| params.get("session_id"))
            .or_else(|| params.get("session"))
            .and_then(Value::as_str)
            .unwrap_or("default");
        let rendered = self.dispatcher.call(&name, &args, session);
        let is_error = rendered.is_error || !rendered.ok;
        Ok(json!({
            "content": [{"type": "text", "text": rendered.text}],
            "isError": is_error
        }))
    }
}

pub fn serve_stdio(
    root: &Path,
    config_path: Option<&Path>,
    no_project_config: bool,
    tier: Option<Tier>,
) -> Result<()> {
    let mut server = McpServer::open(root, config_path, no_project_config, tier)?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => return Err(WErr::internal(format!("stdin: {e}"))),
        };
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("Content-Length:")
            || line.starts_with("Content-Type:")
        {
            continue;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let err = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {"code": -32700, "message": format!("parse error: {e}")}
                });
                writeln!(out, "{err}").map_err(|e| WErr::internal(format!("stdout: {e}")))?;
                out.flush().ok();
                continue;
            }
        };
        if let Some(resp) = server.handle(&msg) {
            writeln!(out, "{resp}").map_err(|e| WErr::internal(format!("stdout: {e}")))?;
            out.flush().ok();
        }
    }
    Ok(())
}

pub fn default_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
