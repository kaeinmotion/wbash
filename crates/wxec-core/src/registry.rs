use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use wxec_index::{SymbolIndex, Workspace};

use crate::config::Config;
use crate::error::{Result, WErr};
use crate::handles::HandleStore;
use crate::parsers::ParserSet;
use crate::policy;
use crate::redact::Redactor;
use crate::types::{Ledger, Tier, ToolResult};

pub struct ToolContext<'a> {
    pub root: &'a Path,
    pub registry: &'a Registry,
    pub tier: Tier,
    pub config: &'a Config,
    pub workspace: &'a Workspace,
    pub symbols: &'a SymbolIndex,
    pub parsers: &'a dyn ParserSet,
    pub handles: &'a mut HandleStore,
    pub ledger: &'a mut Ledger,
    pub redactor: &'a Redactor,
    pub session_id: &'a str,
    pub budget: u32,
    pub allowlist: &'a [PathBuf],
}

impl ToolContext<'_> {
    pub fn resolve(&self, input: &str, must_exist: bool) -> Result<PathBuf> {
        policy::resolve(self.root, input, self.allowlist, must_exist)
    }

    pub fn rel(&self, path: &Path) -> String {
        self.workspace.rel(path)
    }
}

/// Tools return structured results; only the formatter writes wire text.
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn min_tier(&self) -> Tier;
    fn destructive(&self) -> bool {
        false
    }
    fn idempotent(&self) -> bool {
        true
    }
    fn schema(&self) -> Value;
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult>;
}

pub struct Registry {
    tools: BTreeMap<&'static str, Arc<dyn Tool>>,
}

impl Registry {
    pub fn new(tools: Vec<Arc<dyn Tool>>) -> Self {
        let mut map = BTreeMap::new();
        for t in tools {
            map.insert(t.name(), t);
        }
        Self { tools: map }
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        if let Some(t) = self.tools.get(name) {
            return Some(t.clone());
        }
        let canonical = match name {
            "cat" | "read_file" | "read_file_content" | "view" | "open" => "read",
            "grep" | "find_in_files" | "search_files" | "rg" => "search",
            "ls" | "list_dir" | "list_files" | "directory_list" => "tree",
            "write_file" | "write" | "create_file" => "edit",
            "test" | "tests" | "clippy" | "build" => "check",
            "exec" | "execute" | "bash" | "shell" => "run",
            other => other,
        };
        self.tools.get(canonical).cloned()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.tools.keys().copied().collect()
    }

    /// MCP `tools/list` payload. Descriptions are deliberately terse: the
    /// whole tool surface MUST stay within the schema-token budget (G5).
    pub fn mcp_tools(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|t| {
                json!({
                    "name": t.name(),
                    "description": t.description(),
                    "inputSchema": t.schema(),
                    "annotations": {
                        "readOnlyHint": t.min_tier() == Tier::ReadOnly && !matches!(t.name(), "git" | "batch"),
                        "destructiveHint": t.destructive(),
                        "idempotentHint": t.idempotent(),
                        "openWorldHint": matches!(t.name(), "run" | "sh")
                    }
                })
            })
            .collect()
    }

    /// Compact schema surface used by the G5 schema-token gate.
    pub fn schema_surface(&self) -> String {
        let mut text = String::new();
        for t in self.tools.values() {
            text.push_str(t.name());
            text.push('\n');
            text.push_str(t.description());
            text.push('\n');
            text.push_str(&t.schema().to_string());
            text.push('\n');
        }
        text
    }
}

/// Common validation + policy applied to every invocation, including batch
/// children: tier/gates, then path jail for path-like args.
pub fn precheck(
    registry: &Registry,
    name: &str,
    args: &Value,
    ctx: &ToolContext<'_>,
) -> Result<Arc<dyn Tool>> {
    let tool = registry.get(name).ok_or_else(|| {
        WErr::with_hint(
            "invalid_args",
            format!("unknown tool '{name}'"),
            format!("known tools: {}", registry.names().join(", ")),
        )
    })?;
    policy::check_tier(ctx.tier, tool.min_tier(), name)?;
    if name == "sh" && !ctx.config.exec.allow_shell {
        return Err(WErr::with_hint(
            "policy_denied",
            "shell execution is disabled",
            "set exec.allow_shell=true in wxec config to enable `sh`",
        ));
    }
    jail_args(args, ctx)?;
    Ok(tool)
}

fn jail_args(args: &Value, ctx: &ToolContext<'_>) -> Result<()> {
    let Some(obj) = args.as_object() else {
        return Ok(());
    };
    for key in ["path", "cwd"] {
        if let Some(Value::String(s)) = obj.get(key) {
            ctx.resolve(s, false)?;
        }
    }
    match obj.get("pathspec") {
        Some(Value::String(s)) => {
            ctx.resolve(s, false)?;
        }
        Some(Value::Array(items)) => {
            for item in items {
                if let Value::String(s) = item {
                    // Git pathspecs are relative globs; jail the literal prefix.
                    let base = s.split(['*', '?', '[']).next().unwrap_or(s);
                    if !base.is_empty() {
                        ctx.resolve(base, false)?;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// Execute a tool with policy checks. Used by the dispatcher and by `batch`.
pub fn invoke_tool(
    registry: &Registry,
    name: &str,
    args: &Value,
    ctx: &mut ToolContext<'_>,
) -> Result<ToolResult> {
    let tool = precheck(registry, name, args, ctx)?;
    tool.call(args, ctx)
}
