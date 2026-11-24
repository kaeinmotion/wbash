use serde::Deserialize;
use serde_json::{json, Value};

use wxec_core::args::parse_args;
use wxec_core::error::{Result, WErr};
use wxec_core::estimator;
use wxec_core::formatter::{self, RenderOptions};
use wxec_core::registry::{invoke_tool, precheck, Tool, ToolContext};
use wxec_core::types::{Format, HandleRef, Item, ItemClass, Tier, ToolResult};

pub struct BatchTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchArgs {
    #[serde(
        alias = "tools",
        alias = "operations",
        alias = "commands",
        alias = "steps",
        alias = "actions"
    )]
    calls: Vec<BatchCall>,
    #[serde(default, alias = "fail_fast", alias = "bail_on_error")]
    stop_on_error: bool,
    #[serde(default, alias = "quiet", alias = "rollup", alias = "summary")]
    compact: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchCall {
    #[serde(alias = "name")]
    tool: String,
    #[serde(
        default = "empty_obj",
        alias = "arguments",
        alias = "params",
        alias = "parameters",
        alias = "input"
    )]
    args: Value,
}

fn empty_obj() -> Value {
    json!({})
}

impl Tool for BatchTool {
    fn name(&self) -> &'static str {
        "batch"
    }
    fn description(&self) -> &'static str {
        "Run multiple tools in one turn."
    }
    fn min_tier(&self) -> Tier {
        Tier::ReadOnly
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "calls": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": {"type": "string"},
                            "args": {"type": "object"}
                        },
                        "required": ["tool"],
                        "additionalProperties": false
                    }
                },
                "stop_on_error": {"type": "boolean"},
                "compact": {"type": "boolean"}
            },
            "required": ["calls"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let mut a: BatchArgs = parse_args(
            args.clone(),
            "batch",
            "calls=[{\"tool\":\"read\",\"args\":{...}}]",
        )?;
        if a.calls.is_empty() {
            return Err(WErr::invalid_args(
                "calls must not be empty",
                "calls=[{tool,args}]",
            ));
        }
        let max = ctx.config.batch.max_calls;
        if a.calls.len() > max {
            return Err(WErr::invalid_args(
                format!("too many calls ({}, max {max})", a.calls.len()),
                format!("split into batches of at most {max}"),
            ));
        }
        for (i, call) in a.calls.iter_mut().enumerate() {
            if call.tool == "batch" {
                return Err(WErr::invalid_args(
                    "nested batch is not supported",
                    format!("call #{i} is batch"),
                ));
            }
            if call.args.is_null() {
                call.args = json!({});
            }
            precheck(ctx.registry, &call.tool, &call.args, ctx)?;
        }

        let n = a.calls.len();
        let mut result = ToolResult::new("batch", String::new());
        let mut ok_count = 0usize;
        let mut err_count = 0usize;
        let mut used_items_tokens = 0u32;
        let mut total_ins = 0i64;
        let mut total_del = 0i64;
        let mut total_passed = 0i64;
        let mut total_replacements = 0i64;
        let mut last_file_hashes: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (i, call) in a.calls.iter().enumerate() {
            let mut call_args = call.args.clone();
            if call.tool == "edit" {
                if let Some(path_val) = call_args.get("path").and_then(|v| v.as_str()) {
                    let norm_path = path_val.strip_prefix("file://").unwrap_or(path_val);
                    if let Some(prev_hash) = last_file_hashes.get(norm_path) {
                        if let Some(obj) = call_args.as_object_mut() {
                            if obj.contains_key("expect_hash") {
                                obj.insert(
                                    "expect_hash".to_string(),
                                    serde_json::Value::String(prev_hash.clone()),
                                );
                            }
                        }
                    }
                }
            }
            let remaining = n.saturating_sub(i) as u32;
            let used = estimator::estimate_text(&result.header).saturating_add(used_items_tokens);
            let budget_left = ctx.budget.saturating_sub(used);
            let per_child = (budget_left.saturating_sub(24) / remaining.max(1)).max(48);
            match invoke_tool(ctx.registry, &call.tool, &call_args, ctx) {
                Ok(child) => {
                    ok_count += 1;
                    if let Some(h) = child.meta.strings.get("hash") {
                        if let Some(p) = call.args.get("path").and_then(|v| v.as_str()) {
                            let norm_path = p.strip_prefix("file://").unwrap_or(p);
                            last_file_hashes.insert(norm_path.to_string(), h.clone());
                        }
                    }
                    if let Some(&ins) = child.meta.counts.get("inserted") {
                        total_ins += ins;
                    }
                    if let Some(&del) = child.meta.counts.get("deleted") {
                        total_del += del;
                    }
                    if let Some(&p) = child.meta.counts.get("passed") {
                        total_passed += p;
                    }
                    if let Some(&rep) = child.meta.counts.get("replacements") {
                        total_replacements += rep;
                    }
                    let rendered = formatter::render(
                        &child,
                        RenderOptions {
                            budget: per_child,
                            format: Format::Lean,
                            handles: ctx.handles,
                            redactor: ctx.redactor,
                            passthrough_tokens: Some(ctx.config.output.passthrough_tokens),
                            min_gain_pct: Some(ctx.config.output.min_gain_pct),
                            verbosity: Some(ctx.config.output.verbosity.clone()),
                        },
                    )?;
                    let header = format!("#{} {} ok", i + 1, call.tool);
                    let has_text = !rendered.text.trim().is_empty();
                    if has_text || !child.handles.is_empty() {
                        let lines: Vec<&str> = rendered.text.lines().collect();
                        if lines.len() == 1 {
                            let combined = format!("{header}: {}", lines[0]);
                            used_items_tokens = used_items_tokens
                                .saturating_add(estimator::estimate_text(&combined));
                            result.push(Item::new(ItemClass::High, 5.0, vec![combined]));
                        } else {
                            used_items_tokens =
                                used_items_tokens.saturating_add(estimator::estimate_text(&header));
                            used_items_tokens = used_items_tokens
                                .saturating_add(estimator::estimate_text(&rendered.text));
                            let line_strings: Vec<String> =
                                lines.iter().map(|s| (*s).to_string()).collect();
                            result.push(Item::new(ItemClass::High, 5.0, vec![header]));
                            result.push(Item::new(ItemClass::Low, 5.0, line_strings));
                        }
                    }
                    for h in &child.handles {
                        if !result.handles.iter().any(|x| x.id == h.id) {
                            result.handle(HandleRef {
                                id: h.id.clone(),
                                kind: h.kind.clone(),
                                bytes: h.bytes,
                                hash: h.hash.clone(),
                            });
                        }
                    }
                }
                Err(e) => {
                    err_count += 1;
                    let err_hdr = format!("#{} {} error", i + 1, call.tool);
                    let err_msg = e.render();
                    used_items_tokens =
                        used_items_tokens.saturating_add(estimator::estimate_text(&err_hdr));
                    used_items_tokens =
                        used_items_tokens.saturating_add(estimator::estimate_text(&err_msg));
                    result.push(Item::new(ItemClass::MustKeep, 1000.0, vec![err_hdr]));
                    result.must(err_msg);
                    if a.stop_on_error {
                        break;
                    }
                }
            }
        }

        let executed = ok_count + err_count;
        let skipped = n.saturating_sub(executed);
        if skipped > 0 {
            result.header =
                format!("{ok_count} ok, {err_count} error, {skipped} skipped, {n} total");
            result.meta.counts.insert("skipped".into(), skipped as i64);
        } else {
            result.header = format!("{ok_count} ok, {err_count} error, {n} total");
        }
        result.meta.ok = err_count == 0;
        result.meta.counts.insert("ok".into(), ok_count as i64);
        result.meta.counts.insert("error".into(), err_count as i64);
        result.meta.counts.insert("total".into(), n as i64);
        let only_actions = a
            .calls
            .iter()
            .all(|c| matches!(c.tool.as_str(), "edit" | "check" | "git" | "replace"));
        if (a.compact || only_actions) && err_count == 0 {
            result.items.clear();
            let mut parts = Vec::new();
            if total_ins > 0 || total_del > 0 {
                parts.push(format!("+{total_ins} -{total_del}"));
            }
            if total_passed > 0 {
                parts.push(format!("{total_passed} passed"));
            }
            if total_replacements > 0 {
                parts.push(format!("{total_replacements} replaced"));
            }
            let summary = if parts.is_empty() {
                format!("{ok_count} actions ok")
            } else {
                format!("{ok_count} ok ({})", parts.join(", "))
            };
            result.header = format!("batch rollup: {summary}");
        }
        Ok(result)
    }
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(BatchTool));
}
