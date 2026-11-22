use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use wxec_core::args::parse_args;
use wxec_core::error::{Result, WErr};
use wxec_core::executor::{self, ExecOutput};
use wxec_core::parsers::ParsedRun;
use wxec_core::registry::{Tool, ToolContext};
use wxec_core::types::{format_duration_ms, Exit, HandleRef, Tier, ToolResult};

use crate::util;

pub struct ExecRun {
    pub output: ExecOutput,
    pub parsed: Option<ParsedRun>,
    pub injected: Vec<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn execute_with_parsers(
    ctx: &ToolContext<'_>,
    argv: &[String],
    cwd: &Path,
    timeout_ms: u64,
    stdin: Option<&[u8]>,
    env: Option<&HashMap<String, String>>,
    inject: bool,
) -> Result<ExecRun> {
    if argv.is_empty() {
        return Err(WErr::invalid_args(
            "argv must not be empty",
            "argv=[\"prog\", \"arg\"]",
        ));
    }
    if timeout_ms == 0 || timeout_ms > ctx.config.exec.max_timeout_ms {
        return Err(WErr::invalid_args(
            format!("timeout_ms {timeout_ms} out of range"),
            format!("timeout_ms must be 1..={}", ctx.config.exec.max_timeout_ms),
        ));
    }
    let injected = if inject {
        ctx.parsers.inject_args(argv)
    } else {
        argv.to_vec()
    };
    let env_map = executor::build_env(ctx.config, env)?;
    let req = executor::ExecRequest {
        argv: injected.clone(),
        cwd: cwd.to_path_buf(),
        timeout_ms,
        stdin: stdin.map(|s| s.to_vec()),
        env: env_map,
        max_output_bytes: ctx.config.exec.max_output_bytes,
        kill_grace_ms: ctx.config.exec.kill_grace_ms,
    };
    let output = executor::execute(&req)?;
    let raw_machine = output.combined();
    let raw_machine_stdout = output.stdout_str();
    let raw_machine_stderr = output.stderr_str();
    let parsed = if inject {
        ctx.parsers.parse(
            &injected,
            &output.exit,
            &raw_machine_stdout,
            &raw_machine_stderr,
        )
    } else {
        None
    };
    let raw_user = if injected != argv {
        filter_injected_output(&raw_machine, argv, &injected)
    } else {
        raw_machine.clone()
    };
    let mut user_output = output;
    if injected != argv {
        // Prefer user-visible stream; fall back to machine if stripping removed everything unexpectedly.
        if !raw_user.trim().is_empty() || raw_machine.trim().is_empty() {
            user_output.stdout = raw_user.into_bytes();
            user_output.stderr.clear();
        }
    }
    Ok(ExecRun {
        output: user_output,
        parsed,
        injected,
    })
}

fn filter_injected_output(raw: &str, argv: &[String], injected: &[String]) -> String {
    let cargo_json = argv.first().map(|s| s == "cargo").unwrap_or(false)
        && injected.iter().any(|a| a == "--message-format=json");
    if !cargo_json {
        return raw.to_string();
    }
    let mut out: Vec<&str> = Vec::new();
    for line in raw.lines() {
        let t = line.trim_start();
        if t.starts_with('{') && t.ends_with('}') {
            // Drop injected cargo JSONL machine records from user-visible output.
            // libtest/human lines are preserved; parser consumes machine stream separately.
            continue;
        }
        out.push(line);
    }
    let mut s = out.join("\n");
    if raw.ends_with('\n') && !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

pub fn record_output(
    result: &mut ToolResult,
    ctx: &mut ToolContext<'_>,
    output: &ExecOutput,
    parsed: Option<&ParsedRun>,
    label: &str,
    inject_full: bool,
) -> Result<()> {
    let raw = output.combined();
    result.meta.raw_bytes = Some(raw.len() as u64);
    result.meta.duration_ms = Some(output.duration_ms);
    result.meta.exit = Some(output.exit);
    result.meta.ok = output.exit.is_success();

    if output.truncated {
        result.note("output_truncated");
    }
    if output.timed_out {
        result.note("timed_out");
    }

    result.raw_text = Some(raw.clone());
    let mut full_ref: Option<String> = None;
    let has_structured_pre = if let Some(p) = parsed {
        let (_, failed, _) = p.total_counts();
        let errors = p
            .diagnostics
            .iter()
            .filter(|d| d.severity == wxec_core::types::Severity::Error)
            .count();
        errors > 0
            || failed > 0
            || !p.tests.is_empty()
            || p.summary
                .as_ref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
    } else {
        false
    };
    let parse_miss_pre = !output.exit.is_success() && !has_structured_pre && !raw.trim().is_empty();
    let structured_failure_tail =
        !output.exit.is_success() && has_structured_pre && !raw.trim().is_empty();
    // Keep a handle for lossless access when output is non-trivial or we need to show omitted/raw context.
    let minimal_success = output.exit.is_success() && ctx.config.output.verbosity != "verbose";
    let need_handle = !raw.trim().is_empty()
        && !minimal_success
        && (inject_full
            || raw.len() > 4096
            || (!output.exit.is_success() && !structured_failure_tail)
            || (output.exit.is_success() && parsed.is_some()))
        && !parse_miss_pre
        && !structured_failure_tail;
    if need_handle {
        let h = ctx.handles.put(label, raw.as_bytes(), "text")?;
        result.handle(HandleRef {
            id: h.id.clone(),
            kind: h.kind.clone(),
            bytes: h.bytes,
            hash: h.hash.clone(),
        });
        full_ref = Some(h.id);
    }

    if let Some(p) = parsed {
        let (passed, failed, ignored) = p.total_counts();
        let errors = p
            .diagnostics
            .iter()
            .filter(|d| d.severity == wxec_core::types::Severity::Error)
            .count();
        let warnings = p
            .diagnostics
            .iter()
            .filter(|d| d.severity == wxec_core::types::Severity::Warn)
            .count();
        let has_structured = errors > 0
            || failed > 0
            || !p.tests.is_empty()
            || p.summary
                .as_ref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
        let parse_miss = !output.exit.is_success() && !has_structured;
        if let Some(summary) = &p.summary {
            let summary = summary.trim();
            if !summary.is_empty()
                && !result.header.contains(summary)
                && (summary != "0 tests" || !result.header.contains("passed"))
            {
                result.header.push_str(", ");
                result.header.push_str(summary);
            }
        }
        let diags = p.diagnostics.clone();
        util::diagnostics_into(result, diags, ctx.root, ctx.handles)?;
        util::tests_into(result, &p.tests, true);
        if passed > 0 {
            result.meta.counts.insert("passed".into(), passed as i64);
        }
        if failed > 0 {
            result.meta.counts.insert("failed".into(), failed as i64);
        }
        if ignored > 0 {
            result.meta.counts.insert("ignored".into(), ignored as i64);
        }
        if errors > 0 {
            result.meta.counts.insert("errors".into(), errors as i64);
        }
        if warnings > 0 {
            result
                .meta
                .counts
                .insert("warnings".into(), warnings as i64);
        }
        if parse_miss && !raw.trim().is_empty() {
            result.note("parse_miss");
            util::compact_tail(result, &raw, label, ctx.handles, true)?;
            result
                .items
                .retain(|i| !i.lines.iter().any(|l| l.starts_with("full: ~")));
            // single authoritative full handle for parse-miss raw tail
            if let Some(h) = result.handles.last() {
                result.must(format!("full: {}", h.id));
            }
        } else if !output.exit.is_success() && !parse_miss && !raw.trim().is_empty() {
            // Avoid duplicating structured failure facts. When the runner only
            // reported aggregate failures (for example cargo test -q), keep the
            // failing test blocks instead of emitting the whole suite log.
            if failed > 0 && p.tests.is_empty() {
                if !util::compact_test_failures(result, &raw, label, ctx.handles)? {
                    util::compact_tail(result, &raw, label, ctx.handles, false)?;
                }
            } else if failed == 0 && errors == 0 {
                util::compact_tail(result, &raw, label, ctx.handles, false)?;
            }
        }
    } else if !raw.trim().is_empty() {
        if !output.exit.is_success() {
            result.note("parse_miss");
            util::compact_tail(result, &raw, label, ctx.handles, true)?;
            result
                .items
                .retain(|i| !i.lines.iter().any(|l| l.starts_with("full: ~")));
            // single authoritative full handle for parse-miss raw tail
            if let Some(h) = result.handles.last() {
                result.must(format!("full: {}", h.id));
            }
        } else {
            // Success: keep compact. In verbose mode include raw body; otherwise rely on header/counts
            // and emit only a short tail if non-trivial (lossless via handle/raw_text).
            let verbose = ctx.config.output.verbosity == "verbose";
            if verbose {
                for line in raw.lines() {
                    let line = line.trim_end();
                    if !line.is_empty() {
                        result.low(line.to_string());
                    }
                }
            } else {
                // keep at most a few informative summary-like lines with bounded O(1) memory
                let mut ring: std::collections::VecDeque<&str> =
                    std::collections::VecDeque::with_capacity(7);
                let mut count = 0usize;
                for l in raw.lines() {
                    let l = l.trim_end();
                    if !l.is_empty() {
                        count += 1;
                        if ring.len() == 7 {
                            ring.pop_front();
                        }
                        ring.push_back(l);
                    }
                }
                if count > 0 && (count <= 6 || raw.len() <= 200) {
                    for l in ring {
                        result.low(l.to_string());
                    }
                } else {
                    // prefer tail summary lines (e.g. "test result: ok...")
                    let mut matched_any = false;
                    for l in ring.iter().rev().take(4).rev() {
                        if l.contains("test result")
                            || l.contains(" passed")
                            || l.starts_with("ok")
                            || l.starts_with("Finished")
                            || l.contains("warning:")
                            || l.contains("error:")
                            || l.contains("failed")
                        {
                            result.low((*l).to_string());
                            matched_any = true;
                        }
                    }
                    if !matched_any {
                        for l in ring.iter().rev().take(4).rev() {
                            result.low((*l).to_string());
                        }
                    }
                }
            }
        }
    }

    // Structured diagnostics/tests are summarized, but every failing run keeps
    // one full handle so callers can recover unexplored context.
    if !output.exit.is_success() && !raw.trim().is_empty() && result.handles.is_empty() {
        let h = ctx.handles.put(label, raw.as_bytes(), "text")?;
        result.handle(HandleRef {
            id: h.id.clone(),
            kind: h.kind.clone(),
            bytes: h.bytes,
            hash: h.hash.clone(),
        });
    }
    let is_parse_miss = result.meta.notes.iter().any(|n| n.contains("parse_miss"));
    // Ensure lossless handle ref on failures/parse-miss tails even when compaction managed handle.
    let need_full_ref = !output.exit.is_success() || output.truncated || output.timed_out;
    if need_full_ref {
        let has_full = result
            .items
            .iter()
            .any(|i| i.lines.iter().any(|l| l.starts_with("full: ~")));
        if !has_full {
            if let Some(id) = full_ref
                .as_deref()
                .or_else(|| result.handles.last().map(|h| h.id.as_str()))
            {
                result.must(format!("full: {id}"));
            }
        }
    } else if let Some(id) = full_ref {
        let verbose = ctx.config.output.verbosity == "verbose";
        let noisy = result
            .items
            .iter()
            .any(|i| i.class == wxec_core::types::ItemClass::Low && i.lines.len() > 40);
        if verbose
            && noisy
            && !result
                .items
                .iter()
                .any(|i| i.lines.iter().any(|l| l == &format!("full: {id}")))
        {
            result.must(format!("full: {id}"));
        }
    }
    if is_parse_miss {
        // keep exactly one full handle ref (last is authoritative)
        let full_idxs: Vec<usize> = result
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, it)| {
                if it.lines.iter().any(|l| l.starts_with("full: ~")) {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        if full_idxs.len() > 1 {
            let last = *full_idxs.last().unwrap();
            result.items = result
                .items
                .drain(..)
                .enumerate()
                .filter_map(|(i, it)| {
                    if it.lines.iter().any(|l| l.starts_with("full: ~")) {
                        if i == last {
                            Some(it)
                        } else {
                            None
                        }
                    } else {
                        Some(it)
                    }
                })
                .collect();
        }
    }
    Ok(())
}

fn exit_text(exit: &Exit) -> String {
    match exit {
        Exit::Code(c) => format!("exit {c}"),
        Exit::Timeout => "exit timeout".to_string(),
    }
}

// ---------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------

pub struct RunTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunArgs {
    #[serde(
        deserialize_with = "wxec_core::args::deserialize_argv",
        alias = "cmd",
        alias = "args",
        alias = "command",
        alias = "exec"
    )]
    argv: Vec<String>,
    #[serde(default = "dot", alias = "working_dir", alias = "dir", alias = "path")]
    cwd: String,
    timeout_ms: Option<u64>,
    stdin: Option<String>,
    env: Option<HashMap<String, String>>,
}

fn dot() -> String {
    ".".to_string()
}

impl Tool for RunTool {
    fn name(&self) -> &'static str {
        "run"
    }
    fn description(&self) -> &'static str {
        "Execute argv directly; normalized output."
    }
    fn min_tier(&self) -> Tier {
        Tier::Execute
    }
    fn destructive(&self) -> bool {
        true
    }
    fn idempotent(&self) -> bool {
        false
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "argv": {"type": ["array", "string"], "items": {"type": "string"}},
                "cwd": {"type": "string", "default": "."},
                "timeout_ms": {"type": "integer", "minimum": 1},
                "stdin": {"type": "string"},
                "env": {"type": "object", "additionalProperties": {"type": "string"}}
            },
            "required": ["argv"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: RunArgs = parse_args(
            args.clone(),
            "run",
            "pass argv=[\"cmd\", \"args\"] plus optional cwd, timeout_ms, stdin, env",
        )?;
        let cwd = ctx.resolve(&a.cwd, true)?;
        if !cwd.is_dir() {
            return Err(WErr::new(
                "cwd_not_found",
                format!("cwd '{}' is not a directory", a.cwd),
            ));
        }
        let timeout = a.timeout_ms.unwrap_or(ctx.config.exec.timeout_ms);
        let run = execute_with_parsers(
            ctx,
            &a.argv,
            &cwd,
            timeout,
            a.stdin.as_ref().map(|s| s.as_bytes()),
            a.env.as_ref(),
            true,
        )?;
        let mut header = if run.output.duration_ms >= 1000 {
            format!(
                "{}, {}",
                exit_text(&run.output.exit),
                format_duration_ms(run.output.duration_ms)
            )
        } else {
            exit_text(&run.output.exit)
        };
        if run.output.exit.is_success() {
            if let Some(p) = run.parsed.as_ref() {
                let (passed, _failed, ignored) = p.total_counts();
                let warnings = p
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == wxec_core::types::Severity::Warn)
                    .count();
                let summary_lc = p.summary.as_deref().unwrap_or("").to_ascii_lowercase();
                if passed > 0 && !summary_lc.contains("passed") {
                    header.push_str(&format!(", {passed} passed"));
                }
                if ignored > 0 && !summary_lc.contains("ignored") {
                    header.push_str(&format!(", {ignored} ignored"));
                }
                if warnings > 0 && !summary_lc.contains("warning") {
                    header.push_str(&format!(", {}", util::plural(warnings, "warning")));
                }
            }
        }
        let mut result = ToolResult::new("run", header);
        record_output(
            &mut result,
            ctx,
            &run.output,
            run.parsed.as_ref(),
            "run",
            true,
        )?;
        let _ = run.injected;
        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// sh (escape hatch)
// ---------------------------------------------------------------------------

pub struct ShTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShArgs {
    #[serde(alias = "cmd", alias = "c", alias = "exec")]
    command: String,
    #[serde(default = "dot", alias = "working_dir", alias = "dir", alias = "path")]
    cwd: String,
    timeout_ms: Option<u64>,
    stdin: Option<String>,
}

impl Tool for ShTool {
    fn name(&self) -> &'static str {
        "sh"
    }
    fn description(&self) -> &'static str {
        "Run shell command (requires allow_shell); compact."
    }
    fn min_tier(&self) -> Tier {
        Tier::Execute
    }
    fn destructive(&self) -> bool {
        true
    }
    fn idempotent(&self) -> bool {
        false
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "cwd": {"type": "string", "default": "."},
                "timeout_ms": {"type": "integer", "minimum": 1},
                "stdin": {"type": "string"}
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: ShArgs = parse_args(
            args.clone(),
            "sh",
            "pass command plus optional cwd, timeout_ms, stdin",
        )?;
        let cwd = ctx.resolve(&a.cwd, true)?;
        let timeout = a.timeout_ms.unwrap_or(ctx.config.exec.timeout_ms);
        let argv = vec!["/bin/sh".to_string(), "-lc".to_string(), a.command.clone()];
        let run = execute_with_parsers(
            ctx,
            &argv,
            &cwd,
            timeout,
            a.stdin.as_ref().map(|s| s.as_bytes()),
            None,
            false,
        )?;
        let header = if run.output.duration_ms >= 1000 {
            format!(
                "{}, {}",
                exit_text(&run.output.exit),
                format_duration_ms(run.output.duration_ms)
            )
        } else {
            exit_text(&run.output.exit)
        };
        let mut result = ToolResult::new("sh", header);
        record_output(&mut result, ctx, &run.output, None, "sh", true)?;
        result.meta.counts.insert("shell".into(), 1);
        Ok(result)
    }
}

pub fn path_or_root(ctx: &ToolContext<'_>, cwd: &Path) -> PathBuf {
    if cwd.is_dir() {
        cwd.to_path_buf()
    } else {
        ctx.root.to_path_buf()
    }
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(RunTool));
    out.push(std::sync::Arc::new(ShTool));
}
