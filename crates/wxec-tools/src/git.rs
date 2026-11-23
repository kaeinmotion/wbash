use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

use wxec_core::args::{parse_args, parse_line_range};
use wxec_core::error::{Result, WErr};
use wxec_core::policy;
use wxec_core::registry::{Tool, ToolContext};
use wxec_core::types::{format_duration_ms, HandleRef, Tier, ToolResult};

use crate::run::execute_with_parsers;
use crate::util;

pub struct GitTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GitArgs {
    #[serde(
        alias = "operation",
        alias = "subcommand",
        alias = "command",
        alias = "action"
    )]
    op: String,
    #[serde(alias = "paths", alias = "files")]
    pathspec: Option<Value>,
    #[serde(default)]
    staged: bool,
    #[serde(default)]
    cached: bool,
    #[serde(default)]
    stat: bool,
    #[serde(default)]
    name_only: bool,
    #[serde(default)]
    name_status: bool,
    #[serde(default)]
    numstat: bool,
    #[serde(default)]
    shortstat: bool,
    context: Option<u32>,
    limit: Option<u32>,
    #[serde(rename = "ref")]
    reference: Option<String>,
    #[serde(alias = "file", alias = "file_path", alias = "filepath")]
    path: Option<String>,
    #[serde(
        default,
        deserialize_with = "wxec_core::args::deserialize_line_range_opt"
    )]
    lines: Option<String>,
    #[serde(alias = "msg", alias = "m", alias = "summary")]
    message: Option<String>,
    #[serde(default, alias = "auto_stage", alias = "stage_all", alias = "add_all")]
    stage: bool,
    #[serde(default)]
    all: bool,
    #[serde(default)]
    allow_empty: bool,
    remote: Option<String>,
    branch: Option<String>,
    #[serde(default)]
    set_upstream: bool,
    #[serde(default)]
    tags: bool,
    #[serde(default)]
    rebase: bool,
    #[serde(default)]
    args: Option<Vec<String>>,
}

impl Tool for GitTool {
    fn name(&self) -> &'static str {
        "git"
    }
    fn description(&self) -> &'static str {
        "Git status, diff, log, commit, push; compact."
    }
    fn min_tier(&self) -> Tier {
        Tier::ReadOnly
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "op": {"type": "string", "enum": ["status", "add", "diff", "log", "show", "blame", "branch", "commit", "push", "pull", "tag", "remote"]},
                "pathspec": {"type": ["string", "array"]},
                "staged": {"type": "boolean"},
                "stat": {"type": "boolean"},
                "name_only": {"type": "boolean"},
                "numstat": {"type": "boolean"},
                "context": {"type": "integer"},
                "limit": {"type": "integer"},
                "ref": {"type": "string"},
                "path": {"type": "string"},
                "lines": {"type": "string"},
                "message": {"type": "string"},
                "stage": {"type": "boolean"},
                "all": {"type": "boolean"},
                "allow_empty": {"type": "boolean"},
                "remote": {"type": "string"},
                "branch": {"type": "string"},
                "set_upstream": {"type": "boolean"}
            },
            "required": ["op"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: GitArgs = parse_args(
            args.clone(),
            "git",
            "op=status|diff|log|show|blame|branch|commit|push",
        )?;
        ensure_repo(ctx)?;
        match a.op.as_str() {
            "status" => status(ctx),
            "add" => add(ctx, &a),
            "diff" => diff(ctx, &a),
            "log" => log(ctx, &a),
            "show" => show(ctx, &a),
            "blame" => blame(ctx, &a),
            "branch" => branch(ctx, &a),
            "checkout" | "switch" => checkout(ctx, &a),
            "commit" => commit(ctx, &a),
            "push" => push(ctx, &a),
            "pull" => pull(ctx, &a),
            "tag" | "tags" => tag(ctx, &a),
            "remote" | "remotes" => remote(ctx),
            "custom" | "raw" => {
                if let Some(custom_args) = &a.args {
                    policy::check_tier(ctx.tier, Tier::GitWrite, "git custom")?;
                    let refs: Vec<&str> = custom_args.iter().map(String::as_str).collect();
                    let out = git(ctx, &refs).map_err(map_git_err)?;
                    let mut r = ToolResult::new(
                        "git",
                        format!("custom git, {}", format_duration_ms(out.duration_ms)),
                    );
                    r.meta.ok = out.exit.is_success();
                    r.meta.exit = Some(out.exit);
                    let text = out.combined();
                    if !text.trim().is_empty() {
                        let h = ctx.handles.put("git", text.as_bytes(), "text")?;
                        r.handle(wxec_core::types::HandleRef {
                            id: h.id.clone(),
                            kind: h.kind.clone(),
                            bytes: h.bytes,
                            hash: h.hash.clone(),
                        });
                        r.must(format!("full: {}", h.id));
                    }
                    Ok(r)
                } else {
                    Err(WErr::invalid_args(
                        "custom git requires args array",
                        "args=[\"describe\", \"--tags\"]",
                    ))
                }
            }
            other => Err(WErr::with_hint(
                "invalid_args",
                format!("unknown git op '{other}'"),
                "use status|diff|log|show|blame|branch|commit|push|pull|tag|remote",
            )),
        }
    }
}

fn git(ctx: &ToolContext<'_>, args: &[&str]) -> Result<wxec_core::executor::ExecOutput> {
    let argv: Vec<String> = std::iter::once("git".to_string())
        .chain(args.iter().map(|s| s.to_string()))
        .collect();
    let run = execute_with_parsers(
        ctx,
        &argv,
        ctx.root,
        ctx.config.exec.timeout_ms,
        None,
        None,
        false,
    )?;
    Ok(run.output)
}

fn ensure_repo(ctx: &ToolContext<'_>) -> Result<()> {
    let out = git(ctx, &["rev-parse", "--git-dir"])?;
    if !out.exit.is_success() {
        return Err(WErr::with_hint(
            "not_a_repo",
            "not a git repository",
            "run git init first or set --root to the repository",
        ));
    }
    Ok(())
}

fn status(ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
    let out = git(
        ctx,
        &["status", "--porcelain=v1", "--branch", "--no-renames"],
    )
    .map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("status", &out));
    }
    let text = out.stdout_str();
    let mut branch_name = "(detached)".to_string();
    let mut tracking = String::new();
    let mut entries: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            let name = if let Some(n) = rest.strip_prefix("No commits yet on ") {
                n
            } else if let Some(n) = rest.strip_prefix("Initial commit on ") {
                n
            } else {
                rest.split("...")
                    .next()
                    .unwrap_or(rest)
                    .split(' ')
                    .next()
                    .unwrap_or(rest)
            };
            branch_name = name.to_string();
            if let (Some(l), Some(r)) = (rest.find('['), rest.find(']')) {
                if r > l {
                    tracking = format!(" {}", &rest[l..=r]);
                }
            }
        } else if line.len() > 3 {
            let code = line[..2].trim().to_string();
            let path = line[3..].trim().to_string();
            entries.push((code, path));
        }
    }
    let mut r = ToolResult::new(
        "git",
        format!("branch {branch_name}{tracking}, {} changed", entries.len()),
    );
    for (code, path) in &entries {
        let class = if code.contains('U') || code == "AA" || code == "DD" {
            wxec_core::types::ItemClass::MustKeep
        } else {
            wxec_core::types::ItemClass::Low
        };
        r.push(wxec_core::types::Item::new(
            class,
            3.0,
            vec![format!("{code} {path}")],
        ));
    }
    r.meta.counts.insert("changed".into(), entries.len() as i64);
    Ok(r)
}

fn reject_dash_arg(name: &str, val: &str) -> Result<()> {
    if val.starts_with('-') {
        return Err(WErr::invalid_args(
            format!("git {name} must not start with '-'"),
            format!("pass a {name} value, not an option"),
        ));
    }
    Ok(())
}

fn diff(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    let context = a.context.unwrap_or(3);
    if context > ctx.config.search.context_max {
        return Err(WErr::invalid_args(
            format!(
                "context {context} exceeds max {}",
                ctx.config.search.context_max
            ),
            "context=3",
        ));
    }
    let staged = a.staged || a.cached;
    if a.reference.is_some() && staged {
        return Err(WErr::invalid_args(
            "staged/cached cannot be combined with ref",
            "use ref=HEAD or --staged",
        ));
    }
    if let Some(r) = &a.reference {
        reject_dash_arg("ref", r)?;
    }
    let paths = pathspecs(a)?;
    let mut reference = a.reference.clone();
    let mut final_paths = paths.clone();
    if let Some(r) = reference.as_deref() {
        let verify = git(ctx, &["rev-parse", "--revs-only", "--quiet", r]);
        let is_ref =
            matches!(verify, Ok(o) if o.exit.is_success() && !o.stdout_str().trim().is_empty());
        if !is_ref {
            final_paths.insert(0, r.to_string());
            reference = None;
        }
    }
    let summary_mode = a.stat || a.name_only || a.name_status || a.numstat || a.shortstat;
    let mut argv = vec!["diff".to_string(), "--no-color".to_string()];
    if !summary_mode {
        argv.push(format!("--unified={context}"));
    }
    if staged {
        argv.push("--cached".to_string());
    }
    if a.stat {
        argv.push("--stat".to_string());
    }
    if a.numstat {
        argv.push("--numstat".to_string());
    }
    if a.shortstat {
        argv.push("--shortstat".to_string());
    }
    if a.name_only {
        argv.push("--name-only".to_string());
    }
    if a.name_status {
        argv.push("--name-status".to_string());
    }
    if let Some(r) = &reference {
        argv.push(r.clone());
    }
    if !final_paths.is_empty() {
        argv.push("--".to_string());
        argv.extend(final_paths);
    }
    let argv_refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    let out = git(ctx, &argv_refs).map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("diff", &out));
    }
    let text = out.stdout_str();
    let mut r = ToolResult::new("git", String::new());
    if summary_mode {
        let mut files = 0usize;
        let mut plus = 0usize;
        let mut minus = 0usize;
        let mut summary_line = String::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if a.numstat {
                let parts: Vec<&str> = line.splitn(3, '\t').collect();
                if parts.len() == 3 {
                    files += 1;
                    plus += parts[0].parse::<usize>().unwrap_or(0);
                    minus += parts[1].parse::<usize>().unwrap_or(0);
                    r.high(line.to_string());
                    continue;
                }
            }
            if a.shortstat
                || line.contains("file changed")
                || line.contains("files changed")
                || line.contains(" insertion")
                || line.contains(" deletion")
            {
                summary_line = line.trim().to_string();
                continue;
            }
            if a.name_only || a.name_status {
                files += 1;
                r.high(line.to_string());
                continue;
            }
            if a.stat {
                if line.contains(" | ") {
                    files += 1;
                    r.high(line.trim_end().to_string());
                } else {
                    summary_line = line.trim().to_string();
                }
                continue;
            }
            r.low(line.to_string());
        }
        if !summary_line.is_empty() {
            r.header = summary_line;
        } else if text.trim().is_empty() {
            r.header = "0 changed".to_string();
        } else if files > 0 {
            r.header = if a.numstat && (plus > 0 || minus > 0) {
                format!("{files} files, +{plus} -{minus}")
            } else {
                format!("{files} files")
            };
        } else {
            r.header = text
                .lines()
                .next()
                .unwrap_or("0 changed")
                .trim()
                .to_string();
        }
    } else {
        let mut files = 0usize;
        let mut plus = 0usize;
        let mut minus = 0usize;
        let mut seen_file = std::collections::HashSet::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("diff --git ") {
                files += 1;
                let name = rest
                    .split_whitespace()
                    .last()
                    .map(|s| s.trim_start_matches("b/"))
                    .unwrap_or(rest)
                    .to_string();
                if seen_file.insert(name.clone()) {
                    r.high(name);
                }
                continue;
            }
            if line.starts_with("@@") {
                r.high(line.to_string());
                continue;
            }
            if line.starts_with("index ")
                || line.starts_with("+++ ")
                || line.starts_with("--- ")
                || line.starts_with("new file mode")
                || line.starts_with("deleted file mode")
                || line.starts_with("old mode")
                || line.starts_with("new mode")
                || line.starts_with(' ')
            {
                continue;
            }
            if let Some(rest) = line.strip_prefix('+') {
                plus += 1;
                r.low(format!("+{rest}"));
            } else if let Some(rest) = line.strip_prefix('-') {
                minus += 1;
                r.low(format!("-{rest}"));
            } else if line == "\\ No newline at end of file" {
                r.low(line.to_string());
            }
        }
        r.header = if text.trim().is_empty() {
            "0 changed, +0 -0".to_string()
        } else {
            format!("{files} files, +{plus} -{minus}")
        };
    }
    if out.truncated {
        r.note("output_truncated");
    }
    Ok(r)
}

fn log(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    let limit = a.limit.unwrap_or(20).clamp(1, 500);
    let limit_s = format!("-n{limit}");
    let mut args: Vec<String> = vec![
        "log".to_string(),
        "--no-color".to_string(),
        "--date=short".to_string(),
        "--pretty=format:%h%x09%ad%x09%s".to_string(),
        limit_s,
    ];
    if let Some(r) = &a.reference {
        reject_dash_arg("ref", r)?;
        args.push(r.clone());
    }
    let paths = pathspecs(a)?;
    if !paths.is_empty() {
        args.push("--".to_string());
        args.extend(paths);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("log", &out));
    }
    let stdout = out.stdout_str();
    let lines: Vec<&str> = stdout.lines().collect();
    let raw_lines: Vec<String> = lines.iter().map(|l| l.replace('\t', " ")).collect();
    let mut r = ToolResult::new("git", format!("{} commits", lines.len()));
    r.raw_text = Some(raw_lines.join("\n") + if raw_lines.is_empty() { "" } else { "\n" });
    for line in raw_lines {
        r.low(line);
    }
    r.meta.counts.insert("commits".into(), lines.len() as i64);
    Ok(r)
}

fn show(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    let reference = a
        .reference
        .clone()
        .ok_or_else(|| WErr::invalid_args("show requires ref", "ref=HEAD"))?;
    reject_dash_arg("ref", &reference)?;
    let mut args: Vec<String> = vec![
        "show".to_string(),
        "--no-color".to_string(),
        "--pretty=format:%h%x09%ad%x09%an%x09%s".to_string(),
        "--date=short".to_string(),
        reference.clone(),
    ];
    let paths = pathspecs(a)?;
    if !paths.is_empty() {
        args.push("--".to_string());
        args.extend(paths);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("show", &out));
    }
    let text = out.stdout_str();
    let mut r = ToolResult::new("git", format!("show {reference}"));
    let mut files = 0usize;
    let mut plus = 0usize;
    let mut minus = 0usize;
    let mut commit_summary = None;
    for line in text.lines() {
        if commit_summary.is_none() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            commit_summary = Some(t.to_string());
            continue;
        }
        if line.starts_with("diff --git") {
            files += 1;
            let name = line
                .strip_prefix("diff --git ")
                .and_then(|rest| rest.rsplit(" b/").next())
                .unwrap_or(line)
                .trim()
                .to_string();
            r.high(name);
            continue;
        }
        if line.starts_with("@@") {
            r.high(line.to_string());
            continue;
        }
        if line.starts_with("index ")
            || line.starts_with("+++ ")
            || line.starts_with("--- ")
            || line.starts_with("new file ")
            || line.starts_with("deleted file ")
            || line.starts_with("rename ")
            || line.starts_with("old mode ")
            || line.starts_with("new mode ")
            || line.starts_with("similarity index ")
            || line.starts_with("dissimilarity index ")
            || line.starts_with("\\ No newline")
            || line.starts_with(' ')
        {
            continue;
        }
        if line.starts_with('+') {
            plus += 1;
            r.low(line.to_string());
            continue;
        }
        if line.starts_with('-') {
            minus += 1;
            r.low(line.to_string());
            continue;
        }
    }
    if let Some(s) = commit_summary {
        r.must(s);
        if files > 0 {
            r.header = format!(
                "show {reference}, {}, +{plus} -{minus}",
                util::plural(files, "file")
            );
        }
    }
    if out.truncated {
        r.note("output_truncated");
    }
    Ok(r)
}

fn blame(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    let path = a
        .path
        .clone()
        .ok_or_else(|| WErr::invalid_args("blame requires path", "path=src/main.rs"))?;
    let abs = ctx.resolve(&path, true)?;
    let display = ctx.rel(&abs);
    let mut args: Vec<String> = vec!["blame".to_string(), "--line-porcelain".to_string()];
    if let Some(spec) = &a.lines {
        let len = std::fs::read_to_string(&abs)
            .map(|s| s.lines().count())
            .unwrap_or(1);
        let (start, end) = parse_line_range(spec, len)?;
        args.push(format!("-L{start},{end}"));
    }
    args.push("--".to_string());
    args.push(display.clone());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("blame", &out));
    }
    let mut r = ToolResult::new("git", format!("blame {display}"));
    let limit = a
        .limit
        .unwrap_or(ctx.config.search.default_limit)
        .min(ctx.config.search.max_limit) as usize;
    let mut sha = String::new();
    let mut author = String::new();
    let mut final_line = String::new();
    let mut count = 0usize;
    let mut lines_out = Vec::new();
    for line in out.stdout_str().lines() {
        if line.starts_with('\t') {
            count += 1;
            lines_out.push(format!(
                "{final_line} {} {}: {}",
                &sha[..sha.len().min(8)],
                author,
                line.trim_start_matches('\t')
            ));
            continue;
        }
        let mut parts = line.split(' ');
        if let Some(first) = parts.next() {
            if first.len() >= 8 && first.chars().all(|c| c.is_ascii_hexdigit()) {
                sha = first.to_string();
                if let Some(n) = parts.nth(1) {
                    final_line = n.to_string();
                }
                continue;
            }
        }
        if let Some(v) = line.strip_prefix("author ") {
            author = v.to_string();
        }
    }
    let returned = lines_out.len().min(limit);
    for l in lines_out.iter().take(returned) {
        r.low(l.clone());
    }
    if lines_out.len() > returned {
        let omitted = lines_out.len() - returned;
        let body = lines_out[returned..].join("\n");
        let h = ctx.handles.put("git_blame", body.as_bytes(), "text")?;
        r.must(format!("...+{omitted} more ({})", h.id));
        r.handle(HandleRef {
            id: h.id.clone(),
            kind: h.kind.clone(),
            bytes: h.bytes,
            hash: h.hash.clone(),
        });
    }
    r.meta.counts.insert("lines".into(), count as i64);
    Ok(r)
}

fn branch(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    if let Some(b) = &a.branch {
        policy::check_tier(ctx.tier, Tier::GitWrite, "git branch")?;
        reject_dash_arg("branch", b)?;
        let out = git(ctx, &["branch", b.as_str()]).map_err(map_git_err)?;
        let mut r = ToolResult::new("git", format!("created branch {b}"));
        r.meta.ok = out.exit.is_success();
        r.meta.exit = Some(out.exit);
        if !r.meta.ok {
            let errs = extract_git_errors(&out);
            for err in errs {
                r.must(err);
            }
        }
        Ok(r)
    } else {
        let out = git(
            ctx,
            &[
                "branch",
                "--no-color",
                "--format=%(HEAD)%(refname:short)%09%(upstream:short)",
            ],
        )
        .map_err(map_git_err)?;
        if !out.exit.is_success() {
            return Err(git_failure("branch", &out));
        }
        let stdout = out.stdout_str();
        let lines: Vec<&str> = stdout.lines().collect();
        let mut r = ToolResult::new("git", format!("{} branches", lines.len()));
        for line in &lines {
            let (marker, rest) = line.split_at(1.min(line.len()));
            let mut parts = rest.split('\t');
            let name = parts.next().unwrap_or("");
            let upstream = parts.next().unwrap_or("");
            let current = marker == "*";
            let line = if upstream.is_empty() {
                format!("{} {name}", if current { "*" } else { " " })
            } else {
                format!("{} {name} -> {upstream}", if current { "*" } else { " " })
            };
            let class = if current {
                wxec_core::types::ItemClass::High
            } else {
                wxec_core::types::ItemClass::Low
            };
            r.push(wxec_core::types::Item::new(
                class,
                if current { 8.0 } else { 3.0 },
                vec![line],
            ));
        }
        r.meta.counts.insert("branches".into(), lines.len() as i64);
        Ok(r)
    }
}

fn checkout(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    policy::check_tier(ctx.tier, Tier::GitWrite, "git checkout")?;
    let target = a
        .branch
        .as_ref()
        .or(a.reference.as_ref())
        .or(a.path.as_ref())
        .ok_or_else(|| WErr::invalid_args("checkout requires branch or ref", "branch=<name>"))?;
    reject_dash_arg("branch", target)?;
    let mut out = git(ctx, &["checkout", target.as_str()]).map_err(map_git_err)?;
    if !out.exit.is_success() {
        if let Ok(b_out) = git(ctx, &["checkout", "-b", target.as_str()]) {
            if b_out.exit.is_success() {
                out = b_out;
            }
        }
    }
    let mut r = ToolResult::new("git", format!("checkout {target}"));
    r.meta.ok = out.exit.is_success();
    r.meta.exit = Some(out.exit);
    if !r.meta.ok {
        let errs = extract_git_errors(&out);
        for err in errs {
            r.must(err);
        }
    }
    Ok(r)
}

fn add(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    policy::check_tier(ctx.tier, Tier::GitWrite, "git add")?;
    let mut args: Vec<String> = vec!["add".to_string()];
    let paths = pathspecs(a)?;
    if !paths.is_empty() {
        args.push("--".to_string());
        args.extend(paths);
    } else {
        args.push("-A".to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("add", &out));
    }
    let mut r = ToolResult::new("git", "staged changes".to_string());
    r.meta.ok = true;
    Ok(r)
}

fn commit(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    policy::check_tier(ctx.tier, Tier::GitWrite, "git commit")?;
    let message = a
        .message
        .clone()
        .ok_or_else(|| WErr::invalid_args("commit requires message", "message=<commit subject>"))?;
    let mut args: Vec<String> = vec!["commit".to_string(), "-m".to_string(), message];
    if a.all {
        args.push("-a".to_string());
    }
    if a.allow_empty {
        args.push("--allow-empty".to_string());
    }
    let paths = pathspecs(a)?;
    if a.stage {
        let add_out = if !paths.is_empty() {
            let mut add_args = vec!["add".to_string(), "--".to_string()];
            add_args.extend(paths.clone());
            let refs: Vec<&str> = add_args.iter().map(String::as_str).collect();
            git(ctx, &refs).map_err(map_git_err)?
        } else {
            git(ctx, &["add", "-A"]).map_err(map_git_err)?
        };
        if !add_out.exit.is_success() {
            return Err(git_failure("add", &add_out));
        }
    }
    if !paths.is_empty() {
        if a.staged {
            // commit staged subset: ensure index-only commit context
            args.push("--".to_string());
        } else {
            args.push("--".to_string());
        }
        args.extend(paths);
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    let mut r = ToolResult::new(
        "git",
        format!("commit, {}", format_duration_ms(out.duration_ms)),
    );
    if out.exit.is_success() {
        if let Ok(head) = git(ctx, &["rev-parse", "--short", "HEAD"]) {
            let sha = head.stdout_str().trim().to_string();
            if !sha.is_empty() {
                r.header = format!("committed {sha}, {}", format_duration_ms(out.duration_ms));
            }
        }
    } else {
        r.meta.ok = false;
        let errs = extract_git_errors(&out);
        for err in errs {
            r.must(err);
        }
    }
    let text = out.combined();
    if !text.trim().is_empty() {
        let h = ctx.handles.put("git", text.as_bytes(), "text")?;
        r.handle(wxec_core::types::HandleRef {
            id: h.id.clone(),
            kind: h.kind.clone(),
            bytes: h.bytes,
            hash: h.hash.clone(),
        });
        r.must(format!("full: {}", h.id));
        let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
        r.meta.counts.insert("output_lines".into(), lines as i64);
    }
    r.meta.exit = Some(out.exit);
    Ok(r)
}

fn push(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    policy::check_tier(ctx.tier, Tier::GitWrite, "git push")?;
    let mut args: Vec<String> = vec!["push".to_string()];
    if a.tags {
        args.push("--tags".to_string());
    }
    if let Some(remote) = &a.remote {
        reject_dash_arg("remote", remote)?;
        args.push(remote.clone());
    }
    if let Some(branch) = &a.branch {
        reject_dash_arg("branch", branch)?;
        args.push(branch.clone());
    }
    if a.set_upstream {
        args.push("--set-upstream".to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    let first = out
        .stdout_str()
        .lines()
        .chain(out.stderr_str().lines())
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| "push".to_string());
    let mut r = ToolResult::new(
        "git",
        format!("push, {} {first}", format_duration_ms(out.duration_ms)),
    );
    r.meta.ok = out.exit.is_success();
    r.meta.exit = Some(out.exit);
    if !r.meta.ok {
        let errs = extract_git_errors(&out);
        for err in errs {
            r.must(err);
        }
    }
    let text = out.combined();
    let h = ctx.handles.put("git", text.as_bytes(), "text")?;
    r.handle(wxec_core::types::HandleRef {
        id: h.id.clone(),
        kind: h.kind.clone(),
        bytes: h.bytes,
        hash: h.hash.clone(),
    });
    r.must(format!("full: {}", h.id));
    Ok(r)
}

fn pull(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    policy::check_tier(ctx.tier, Tier::GitWrite, "git pull")?;
    let mut args: Vec<String> = vec!["pull".to_string()];
    if a.rebase {
        args.push("--rebase".to_string());
    }
    if let Some(remote) = &a.remote {
        reject_dash_arg("remote", remote)?;
        args.push(remote.clone());
    }
    if let Some(branch) = &a.branch {
        reject_dash_arg("branch", branch)?;
        args.push(branch.clone());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(ctx, &refs).map_err(map_git_err)?;
    let first = out
        .stdout_str()
        .lines()
        .chain(out.stderr_str().lines())
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .unwrap_or_else(|| "pull".to_string());
    let mut r = ToolResult::new(
        "git",
        format!("pull, {} {first}", format_duration_ms(out.duration_ms)),
    );
    r.meta.ok = out.exit.is_success();
    r.meta.exit = Some(out.exit);
    if !r.meta.ok {
        let errs = extract_git_errors(&out);
        for err in errs {
            r.must(err);
        }
    }
    let text = out.combined();
    if !text.trim().is_empty() {
        let h = ctx.handles.put("git", text.as_bytes(), "text")?;
        r.handle(wxec_core::types::HandleRef {
            id: h.id.clone(),
            kind: h.kind.clone(),
            bytes: h.bytes,
            hash: h.hash.clone(),
        });
        r.must(format!("full: {}", h.id));
    }
    Ok(r)
}

fn tag(ctx: &mut ToolContext<'_>, a: &GitArgs) -> Result<ToolResult> {
    let tag_name = a
        .reference
        .as_ref()
        .or(a.path.as_ref())
        .or(a.branch.as_ref());
    if let Some(name) = tag_name {
        policy::check_tier(ctx.tier, Tier::GitWrite, "git tag")?;
        reject_dash_arg("tag name", name)?;
        let mut args = vec!["tag".to_string()];
        if let Some(msg) = &a.message {
            args.push("-a".to_string());
            args.push(name.clone());
            args.push("-m".to_string());
            args.push(msg.clone());
        } else {
            args.push(name.clone());
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = git(ctx, &refs).map_err(map_git_err)?;
        let mut r = ToolResult::new("git", format!("tagged {name}"));
        r.meta.ok = out.exit.is_success();
        r.meta.exit = Some(out.exit);
        if !r.meta.ok {
            let errs = extract_git_errors(&out);
            for err in errs {
                r.must(err);
            }
        }
        Ok(r)
    } else {
        let out = git(ctx, &["tag", "-l", "--sort=-v:refname"]).map_err(map_git_err)?;
        if !out.exit.is_success() {
            return Err(git_failure("tag", &out));
        }
        let text = out.stdout_str();
        let tags: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let mut r = ToolResult::new("git", format!("{} tags", tags.len()));
        let limit = a.limit.unwrap_or(20) as usize;
        for t in tags.iter().take(limit) {
            r.low(t.to_string());
        }
        if tags.len() > limit {
            r.low(format!("...+{} more tags", tags.len() - limit));
        }
        r.meta.counts.insert("tags".into(), tags.len() as i64);
        Ok(r)
    }
}

fn remote(ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
    let out = git(ctx, &["remote", "-v"]).map_err(map_git_err)?;
    if !out.exit.is_success() {
        return Err(git_failure("remote", &out));
    }
    let text = out.stdout_str();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let count = (lines.len() / 2).max(1);
    let mut r = ToolResult::new("git", format!("{count} remotes"));
    for l in lines {
        r.low(l.to_string());
    }
    Ok(r)
}

fn pathspecs(a: &GitArgs) -> Result<Vec<String>> {
    if let Some(spec) = &a.pathspec {
        match spec {
            Value::String(s) => return Ok(vec![s.clone()]),
            Value::Array(items) => {
                let mut out = Vec::new();
                for item in items {
                    match item.as_str() {
                        Some(s) => out.push(s.to_string()),
                        None => {
                            return Err(WErr::invalid_args(
                                "pathspec must be strings",
                                "pathspec=\"src/\"",
                            ))
                        }
                    }
                }
                return Ok(out);
            }
            _ => {
                return Err(WErr::invalid_args(
                    "pathspec must be a string or array",
                    "pathspec=\"src/\"",
                ))
            }
        }
    }
    if let Some(p) = &a.path {
        return Ok(vec![p.clone()]);
    }
    Ok(Vec::new())
}

fn map_git_err(err: WErr) -> WErr {
    if err.code == "exec_not_found" {
        WErr::with_hint(
            "git_error",
            "git executable not found",
            "install git and ensure it is on PATH",
        )
    } else {
        err
    }
}

fn extract_git_errors(out: &wxec_core::executor::ExecOutput) -> Vec<String> {
    let mut errs = Vec::new();
    for line in out.stderr_str().lines().chain(out.stdout_str().lines()) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("error:")
            || trimmed.starts_with("fatal:")
            || trimmed.starts_with("! [rejected]")
            || trimmed.starts_with("CONFLICT")
            || trimmed.contains("did not match any file")
            || trimmed.contains("nothing to commit")
        {
            errs.push(trimmed.to_string());
        }
    }
    if errs.is_empty() {
        for line in out.stderr_str().lines().chain(out.stdout_str().lines()) {
            let trimmed = line.trim();
            if !trimmed.is_empty()
                && !trimmed.starts_with("To ")
                && !trimmed.starts_with("Everything up-to-date")
            {
                errs.push(trimmed.to_string());
                if errs.len() >= 3 {
                    break;
                }
            }
        }
    }
    errs.truncate(5);
    errs
}

fn git_failure(op: &str, out: &wxec_core::executor::ExecOutput) -> WErr {
    let errs = extract_git_errors(out);
    let detail = errs
        .first()
        .cloned()
        .unwrap_or_else(|| "git failed".to_string());
    WErr::with_hint(
        "git_error",
        format!("{op}: {detail}"),
        "inspect with `w git status`",
    )
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(GitTool));
}

pub fn _paths(_: &Path) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxec_core::executor::ExecOutput;
    use wxec_core::types::Exit;

    #[test]
    fn extract_git_errors_prioritizes_fatal_and_rejections() {
        let out = ExecOutput {
            stdout: Vec::new(),
            stderr: b"To github.com:org/repo.git\n ! [rejected] main -> main (fetch first)\nerror: failed to push some refs\nhint: Updates were rejected\n".to_vec(),
            exit: Exit::Code(1),
            duration_ms: 100,
            truncated: false,
            timed_out: false,
        };
        let errs = extract_git_errors(&out);
        assert_eq!(errs.len(), 2);
        assert_eq!(errs[0], "! [rejected] main -> main (fetch first)");
        assert_eq!(errs[1], "error: failed to push some refs");
    }

    #[test]
    fn extract_git_errors_falls_back_when_no_prefixes() {
        let out = ExecOutput {
            stdout: Vec::new(),
            stderr: b"remote: permission denied\nConnection closed\n".to_vec(),
            exit: Exit::Code(1),
            duration_ms: 100,
            truncated: false,
            timed_out: false,
        };
        let errs = extract_git_errors(&out);
        assert_eq!(errs, vec!["remote: permission denied", "Connection closed"]);
    }
}
