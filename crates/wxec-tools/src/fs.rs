use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};

use wxec_core::args::{parse_args, parse_line_range, parse_line_range_clamped};
use wxec_core::error::{Result, WErr};
use wxec_core::hash;
use wxec_core::registry::{Tool, ToolContext};
use wxec_core::types::{
    now_ms, Diagnostic, HandleRef, Item, ItemClass, ReadSeen, Scope, Severity, Tier, ToolResult,
};
use wxec_index::Workspace;

use crate::util;

// ---------------------------------------------------------------------------
// tree
// ---------------------------------------------------------------------------

pub struct TreeTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TreeArgs {
    #[serde(default = "dot", alias = "dir", alias = "directory", alias = "folder")]
    path: String,
    #[serde(default = "two", alias = "max_depth", alias = "level")]
    depth: u32,
    #[serde(alias = "pattern")]
    glob: Option<String>,
    #[serde(default)]
    sizes: bool,
    #[serde(default, alias = "directories_only", alias = "only_dirs")]
    dirs_only: bool,
}

fn dot() -> String {
    ".".to_string()
}
fn two() -> u32 {
    2
}

impl Tool for TreeTool {
    fn name(&self) -> &'static str {
        "tree"
    }
    fn description(&self) -> &'static str {
        "List files as an indented tree."
    }
    fn min_tier(&self) -> Tier {
        Tier::ReadOnly
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "default": "."},
                "depth": {"type": "integer", "minimum": 0, "default": 2},
                "glob": {"type": "string"},
                "sizes": {"type": "boolean", "default": false},
                "dirs_only": {"type": "boolean", "default": false}
            },
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: TreeArgs = parse_args(
            args.clone(),
            "tree",
            "pass path, depth, glob, sizes or dirs_only",
        )?;
        let base = ctx.resolve(&a.path, true)?;
        if !base.is_dir() {
            return Err(WErr::with_hint(
                "not_a_directory",
                format!("'{}' is not a directory", a.path),
                "use `w read path=<file>` for files",
            ));
        }
        let gs = util::globset(a.glob.as_deref())?;
        let base_rel = ctx.rel(&base);
        let walk = ctx.workspace.walk_cached_arc();

        let mut root = DirNode::default();
        for f in walk.files.iter() {
            if !(util::under(&f.rel, &base_rel) && util::matches_glob(gs.as_ref(), &f.rel)) {
                continue;
            }
            let rel = if base_rel == "." {
                f.rel.as_str()
            } else if let Some(stripped) = f.rel.strip_prefix(&format!("{base_rel}/")) {
                stripped
            } else {
                f.rel.as_str()
            };
            if rel.is_empty() {
                continue;
            }
            let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
            if parts.is_empty() {
                continue;
            }
            root.insert(&parts, f.size);
        }

        let mut result = ToolResult::new(
            "tree",
            format!(
                "{}/ ({}, {})",
                util::display_path(ctx.root, &base),
                util::plural(root.file_count(), "file"),
                util::plural(root.dir_count(), "dir")
            ),
        );
        if walk.truncated {
            result.note("tree_truncated");
        }
        let max_depth = if a.depth == 0 {
            usize::MAX
        } else {
            a.depth as usize
        };
        render_tree(&mut result, &root, 0, max_depth, a.sizes, a.dirs_only, ctx)?;
        result
            .meta
            .counts
            .insert("files".into(), root.file_count() as i64);
        result
            .meta
            .counts
            .insert("dirs".into(), root.dir_count() as i64);
        Ok(result)
    }
}

#[derive(Default)]
struct DirNode {
    dirs: std::collections::BTreeMap<String, DirNode>,
    files: Vec<(String, u64)>,
}

impl DirNode {
    fn insert(&mut self, parts: &[&str], size: u64) {
        if parts.len() == 1 {
            self.files.push((parts[0].to_string(), size));
        } else {
            self.dirs
                .entry(parts[0].to_string())
                .or_default()
                .insert(&parts[1..], size);
        }
    }

    fn file_count(&self) -> usize {
        self.files.len() + self.dirs.values().map(DirNode::file_count).sum::<usize>()
    }

    fn dir_count(&self) -> usize {
        self.dirs.len() + self.dirs.values().map(DirNode::dir_count).sum::<usize>()
    }
}

fn render_tree(
    result: &mut ToolResult,
    node: &DirNode,
    level: usize,
    max_depth: usize,
    sizes: bool,
    dirs_only: bool,
    ctx: &mut ToolContext<'_>,
) -> Result<()> {
    let indent = "  ".repeat(level);
    for (name, child) in &node.dirs {
        let count = child.file_count();
        let over_depth = level + 1 >= max_depth;
        let small_leaf = child.dirs.is_empty() && child.files.len() <= 5;
        let collapse =
            (over_depth && !small_leaf) || child.files.len() > 40 || child.dir_count() > 40;
        if collapse {
            let mut lines = Vec::new();
            collect_tree_lines(child, &format!("{indent}  "), sizes, &mut lines);
            if lines.is_empty() {
                result.must(format!("{indent}{name}/ ({count})"));
            } else {
                let h = ctx
                    .handles
                    .put("tree", lines.join("\n").as_bytes(), "text")?;
                result.must(format!("{indent}{name}/ ({count}) {}", h.id));
                result.handle(HandleRef {
                    id: h.id.clone(),
                    kind: h.kind.clone(),
                    bytes: h.bytes,
                    hash: h.hash.clone(),
                });
            }
        } else {
            result.push(Item::new(
                ItemClass::High,
                5.0,
                vec![format!("{indent}{name}/")],
            ));
            render_tree(result, child, level + 1, max_depth, sizes, dirs_only, ctx)?;
        }
    }
    if dirs_only {
        return Ok(());
    }
    let mut files = node.files.clone();
    files.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    const INLINE_MAX: usize = 40;
    if files.len() > INLINE_MAX {
        let label = if sizes {
            format!(
                "{indent}({} files, {})",
                files.len(),
                human_size(files.iter().map(|(_, s)| *s).sum())
            )
        } else {
            format!("{indent}({} files)", files.len())
        };
        let lines: Vec<String> = files
            .iter()
            .map(|(n, s)| {
                if sizes {
                    format!("{indent}{n} ({})", human_size(*s))
                } else {
                    format!("{indent}{n}")
                }
            })
            .collect();
        let h = ctx
            .handles
            .put("tree", lines.join("\n").as_bytes(), "text")?;
        result.must(format!("{label} {}", h.id));
        result.handle(HandleRef {
            id: h.id.clone(),
            kind: h.kind.clone(),
            bytes: h.bytes,
            hash: h.hash.clone(),
        });
    } else {
        for (name, size) in &files {
            let line = if sizes {
                format!("{indent}{name} ({})", human_size(*size))
            } else {
                format!("{indent}{name}")
            };
            result.push(Item::new(ItemClass::High, 3.0, vec![line]));
        }
    }
    Ok(())
}

fn collect_tree_lines(node: &DirNode, indent: &str, sizes: bool, out: &mut Vec<String>) {
    for (name, child) in &node.dirs {
        out.push(format!("{indent}{name}/ ({})", child.file_count()));
        collect_tree_lines(child, &format!("{indent}  "), sizes, out);
    }
    let mut files = node.files.clone();
    files.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    for (name, size) in &files {
        if sizes {
            out.push(format!("{indent}{name} ({})", human_size(*size)));
        } else {
            out.push(format!("{indent}{name}"));
        }
    }
}

fn human_size(n: u64) -> String {
    if n < 1024 {
        format!("{n}B")
    } else if n < 1024 * 1024 {
        format!("{:.1}KB", n as f64 / 1024.0)
    } else {
        format!("{:.1}MB", n as f64 / (1024.0 * 1024.0))
    }
}

// ---------------------------------------------------------------------------
// read
// ---------------------------------------------------------------------------

pub struct ReadTool;

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    #[serde(default, alias = "file", alias = "file_path", alias = "filepath")]
    path: String,
    #[serde(default, alias = "files")]
    paths: Vec<String>,
    #[serde(
        default,
        deserialize_with = "wxec_core::args::deserialize_line_range_opt",
        alias = "range",
        alias = "line_range",
        alias = "line"
    )]
    lines: Option<String>,
    #[serde(alias = "start", alias = "from")]
    start_line: Option<usize>,
    #[serde(alias = "end", alias = "to")]
    end_line: Option<usize>,
    symbol: Option<String>,
    #[serde(default)]
    outline: bool,
    expect_hash: Option<String>,
    #[serde(default)]
    force: bool,
}

impl Tool for ReadTool {
    fn name(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> &'static str {
        "Read file or line range."
    }
    fn min_tier(&self) -> Tier {
        Tier::ReadOnly
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "paths": {"type": "array", "items": {"type": "string"}},
                "lines": {"type": ["string", "array"]},
                "symbol": {"type": "string"},
                "outline": {"type": "boolean", "default": false},
                "expect_hash": {"type": "string"},
                "force": {"type": "boolean", "default": false}
            },
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let mut a: ReadArgs = parse_args(
            args.clone(),
            "read",
            "pass path plus optional lines, symbol, outline, expect_hash, force",
        )?;
        if a.lines.is_none() && (a.start_line.is_some() || a.end_line.is_some()) {
            let s = a.start_line.unwrap_or(1);
            let e = a.end_line.map(|v| v.to_string()).unwrap_or_default();
            a.lines = Some(if e.is_empty() {
                format!("{s}:")
            } else {
                format!("{s}:{e}")
            });
        }
        if !a.paths.is_empty() {
            let mut r = ToolResult::new("read", format!("{} files", a.paths.len()));
            for p in &a.paths {
                let mut single_args = a.clone();
                single_args.path = p.clone();
                single_args.paths.clear();
                let sub =
                    self.call(&serde_json::to_value(&single_args).unwrap_or_default(), ctx)?;
                r.push(Item::new(
                    ItemClass::High,
                    5.0,
                    vec![format!("--- {} ---", sub.header)],
                ));
                for item in sub.items {
                    r.push(item);
                }
                for h in sub.handles {
                    r.handle(h);
                }
            }
            return Ok(r);
        }
        if a.path.is_empty() {
            return Err(WErr::invalid_args(
                "read requires path or paths",
                "read path=<file>",
            ));
        }
        let abs = ctx.resolve(&a.path, true)?;
        if abs.is_dir() {
            return Err(WErr::with_hint(
                "is_directory",
                format!("'{}' is a directory", a.path),
                "use `w tree path=<dir>`",
            ));
        }
        let file = wxec_index::read_text(&abs)
            .map_err(|e| WErr::new("not_found", format!("cannot read '{}': {e}", a.path)))?;
        let full_hex = hash::hash_bytes(file.content.as_bytes());
        let short = hash::short(&full_hex);
        if let Some(expect) = &a.expect_hash {
            let exp = expect.trim_start_matches("b3:");
            let ok = !exp.is_empty()
                && (expect == &short
                    || expect == &full_hex
                    || full_hex.starts_with(exp)
                    || short.trim_start_matches("b3:").starts_with(exp));
            if !ok {
                return Err(WErr::with_hint(
                    "hash_mismatch",
                    format!("current={short}"),
                    "re-read the file before editing",
                ));
            }
        }
        if file.binary {
            if a.symbol.is_some() || a.lines.is_some() || a.outline {
                return Err(WErr::with_hint(
                    "binary_unsupported",
                    format!("'{}' is binary", a.path),
                    "read binary files without lines/symbol/outline filters",
                ));
            }
            let raw = std::fs::read(&abs).unwrap_or_default();
            let full_hex = hash::hash_bytes(&raw);
            let short = hash::short(&full_hex);
            let mut r = ToolResult::new(
                "read",
                format!("binary {} {} bytes {short}", a.path, raw.len()),
            );
            r.meta.counts.insert("bytes".into(), raw.len() as i64);
            return Ok(r);
        }

        let display = ctx.rel(&abs);
        let text = file.content;
        let lines: Vec<&str> = text.lines().collect();
        let line_count = lines.len();
        let mtime_ms = std::fs::metadata(&abs)
            .map(|m| wxec_core::types::file_mtime_ms(&m))
            .unwrap_or(0);

        if let Some(sym) = &a.symbol {
            let found = ctx.symbols.find_symbol(&abs, &text, sym).ok_or_else(|| {
                WErr::with_hint(
                    "symbol_not_found",
                    format!("symbol '{sym}' in {display}"),
                    format!("try: w search pattern={sym} mode=symbol"),
                )
            })?;
            let start = found.start_line.max(1).min(line_count.max(1));
            let end = found.end_line.max(start).min(line_count.max(1));
            let mut r = ToolResult::new(
                "read",
                format!("{display}:{start}-{end} {} ({short})", found.name),
            );
            for i in start..=end {
                r.low(format!("{i}: {}", lines[i - 1]));
            }
            r.meta
                .counts
                .insert("lines".into(), (end - start + 1) as i64);
            record_read(
                ctx,
                &display,
                Scope::Lines {
                    start: start as u32,
                    end: end as u32,
                },
                &full_hex,
                &short,
                mtime_ms,
                file.size,
                &text,
                Some(&found.name),
            )?;
            return Ok(r);
        }

        if a.outline {
            let outline = ctx.symbols.symbols(&abs, &text);
            let mut r = ToolResult::new(
                "read",
                format!(
                    "{display} outline ({} symbols, {short})",
                    outline.symbols.len()
                ),
            );
            if outline.heuristic {
                r.note("heuristic_outline");
            }
            for s in &outline.symbols {
                let pad = " ".repeat(s.indent.min(40));
                r.low(format!("{pad}{}: {} {}", s.start_line, s.kind, s.name));
            }
            r.meta
                .counts
                .insert("symbols".into(), outline.symbols.len() as i64);
            record_read(
                ctx,
                &display,
                Scope::Outline,
                &full_hex,
                &short,
                mtime_ms,
                file.size,
                &text,
                None,
            )?;
            return Ok(r);
        }

        if let Some(spec) = &a.lines {
            let (start, end) = parse_line_range_clamped(spec, line_count)?;
            let count = end - start + 1;
            let header = if a.expect_hash.is_some() {
                format!("{display}:{start}-{end} ({count} lines, {short})")
            } else {
                format!("{display}:{start}-{end} ({count} lines)")
            };
            let mut r = ToolResult::new("read", header);
            if spec_mismatch(spec, start, end, line_count) {
                r.note(format!("clamped {spec} -> {start}:{end}"));
            }
            let mut raw = String::new();
            for i in start..=end {
                let line = format!("{i}: {}", lines[i - 1]);
                raw.push_str(&line);
                raw.push('\n');
                r.push(Item::new(ItemClass::MustKeep, 1.0, vec![line]));
            }
            r.raw_text = Some(raw);
            r.meta.counts.insert("lines".into(), count as i64);
            record_read(
                ctx,
                &display,
                Scope::Lines {
                    start: start as u32,
                    end: end as u32,
                },
                &full_hex,
                &short,
                mtime_ms,
                file.size,
                &text,
                None,
            )?;
            return Ok(r);
        }

        let max_full = ctx.config.read.max_full_lines as usize;
        let scope = if line_count <= max_full {
            Scope::Full
        } else {
            Scope::Lines {
                start: 1,
                end: ctx.config.read.head_lines,
            }
        };
        let ttl = ctx.config.session.ledger_ttl_secs;
        let now = now_ms();
        let previous = if a.force {
            None
        } else {
            ctx.ledger.read_entry(&display, scope, ttl, now).cloned()
        };
        if let Some(prev) = &previous {
            if prev.full_hash == full_hex {
                if let Some(snap) = prev
                    .snapshot_ref
                    .as_ref()
                    .filter(|id| ctx.handles.get(id).is_ok())
                {
                    let mut r = ToolResult::new(
                        "read",
                        format!("unchanged {display} ({short}) expand {snap}"),
                    );
                    r.meta.counts.insert("lines".into(), line_count as i64);
                    return Ok(r);
                }
            }
        }

        let mut r = ToolResult::new(
            "read",
            if line_count <= max_full {
                format!("{display} ({line_count} lines, {short})")
            } else {
                format!("{display} ({line_count} lines, head+outline, {short})")
            },
        );

        if line_count > max_full {
            let head = (ctx.config.read.head_lines as usize).min(line_count);
            for (i, line) in lines.iter().take(head).enumerate() {
                r.push(Item::new(
                    ItemClass::High,
                    6.0,
                    vec![format!("{}: {}", i + 1, line)],
                ));
            }
            let h = ctx.handles.put("read", text.as_bytes(), "text")?;
            r.must(format!("...+{} lines ({})", line_count - head, h.id));
            r.handle(HandleRef {
                id: h.id.clone(),
                kind: h.kind.clone(),
                bytes: h.bytes,
                hash: h.hash.clone(),
            });
            let outline = ctx.symbols.symbols(&abs, &text);
            if outline.heuristic {
                r.note("heuristic_outline");
            }
            let mut outline_shown = 0usize;
            let mut outline_skipped = 0usize;
            const MAX_OUTLINE_LINES: usize = 50;
            for s in &outline.symbols {
                if s.start_line <= head {
                    continue;
                }
                if outline_shown >= MAX_OUTLINE_LINES {
                    outline_skipped += 1;
                    continue;
                }
                let pad = " ".repeat(s.indent.min(40));
                r.push(Item::new(
                    ItemClass::High,
                    4.0,
                    vec![format!("{pad}{}: {} {}", s.start_line, s.kind, s.name)],
                ));
                outline_shown += 1;
            }
            if outline_skipped > 0 {
                r.low(format!("+{outline_skipped} more symbols ({})", h.id));
            }
            record_read_snapshot(
                ctx,
                &display,
                scope,
                &full_hex,
                &short,
                mtime_ms,
                file.size,
                &text,
                Some(h.id),
            )?;
            r.meta.counts.insert("lines".into(), line_count as i64);
            return Ok(r);
        }

        let full_text_est = wxec_core::estimator::estimate_text(&text);
        let mut used_diff = false;
        if let Some(prev) = &previous {
            if let Some(snap) = &prev.snapshot_ref {
                if let Ok((bytes, _)) = ctx.handles.get(snap) {
                    if let Ok(old) = std::str::from_utf8(&bytes) {
                        let diff_lines = render_diff(old, &text);
                        if wxec_core::estimator::estimate_lines(&diff_lines) < full_text_est {
                            used_diff = true;
                            r.header = format!("{display} diff ({short})");
                            for line in diff_lines {
                                r.push(Item::new(ItemClass::High, 6.0, vec![line]));
                            }
                        }
                    } else {
                        let old = String::from_utf8_lossy(&bytes);
                        let diff_lines = render_diff(&old, &text);
                        if wxec_core::estimator::estimate_lines(&diff_lines) < full_text_est {
                            used_diff = true;
                            r.header = format!("{display} diff ({short})");
                            for line in diff_lines {
                                r.push(Item::new(ItemClass::High, 6.0, vec![line]));
                            }
                        }
                    }
                }
            }
        }
        if !used_diff {
            for (i, line) in lines.iter().enumerate() {
                r.low(format!("{}: {}", i + 1, line));
            }
        }
        let snapshot = ctx.handles.put("read_snapshot", text.as_bytes(), "text")?;
        record_read_snapshot(
            ctx,
            &display,
            scope,
            &full_hex,
            &short,
            mtime_ms,
            file.size,
            &text,
            Some(snapshot.id),
        )?;
        r.meta.counts.insert("lines".into(), line_count as i64);
        Ok(r)
    }
}

fn spec_mismatch(spec: &str, start: usize, end: usize, len: usize) -> bool {
    let (a, b) = spec
        .split_once(':')
        .or_else(|| spec.split_once(".."))
        .or_else(|| spec.split_once('-'))
        .or_else(|| spec.split_once(','))
        .unwrap_or(("", ""));
    let req_start = a.trim().parse::<usize>().unwrap_or(1);
    let req_end = b.trim().parse::<usize>().unwrap_or(len);
    req_start != start || req_end != end
}

#[allow(clippy::too_many_arguments)]
fn record_read(
    ctx: &mut ToolContext<'_>,
    display: &str,
    scope: Scope,
    full_hex: &str,
    short: &str,
    mtime_ms: i64,
    size: u64,
    text: &str,
    _symbol: Option<&str>,
) -> Result<()> {
    let snapshot = ctx.handles.put("read_snapshot", text.as_bytes(), "text")?;
    record_read_snapshot(
        ctx,
        display,
        scope,
        full_hex,
        short,
        mtime_ms,
        size,
        text,
        Some(snapshot.id),
    )
}

#[allow(clippy::too_many_arguments)]
fn record_read_snapshot(
    ctx: &mut ToolContext<'_>,
    display: &str,
    scope: Scope,
    full_hex: &str,
    short: &str,
    mtime_ms: i64,
    size: u64,
    _text: &str,
    snapshot_ref: Option<String>,
) -> Result<()> {
    ctx.ledger.record_read(
        display,
        scope,
        ReadSeen {
            full_hash: full_hex.to_string(),
            short: short.to_string(),
            mtime_ms,
            size,
            scope: scope.key(),
            snapshot_ref,
            shown_ms: now_ms(),
        },
    );
    Ok(())
}

fn render_diff(old: &str, new: &str) -> Vec<String> {
    let diff = TextDiff::from_lines(old, new);
    let mut out = Vec::new();
    let mut old_line;
    let mut new_line;
    for group in diff.grouped_ops(3) {
        let Some(first) = group.first() else { continue };
        old_line = first.old_range().start + 1;
        new_line = first.new_range().start + 1;
        let old_start = first.old_range().start + 1;
        let old_len = group.iter().map(|op| op.old_range().len()).sum::<usize>();
        let new_start = first.new_range().start + 1;
        let new_len = group.iter().map(|op| op.new_range().len()).sum::<usize>();
        out.push(format!(
            "@@ -{old_start},{old_len} +{new_start},{new_len} @@"
        ));
        for op in group {
            for change in diff.iter_changes(&op) {
                let value = change.value().trim_end_matches('\n');
                match change.tag() {
                    ChangeTag::Delete => {
                        out.push(format!("-{old_line}: {value}"));
                        old_line += 1;
                    }
                    ChangeTag::Insert => {
                        out.push(format!("+{new_line}: {value}"));
                        new_line += 1;
                    }
                    ChangeTag::Equal => {
                        out.push(format!(" {new_line}: {value}"));
                        old_line += 1;
                        new_line += 1;
                    }
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// edit
// ---------------------------------------------------------------------------

pub struct EditTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    #[serde(alias = "file", alias = "file_path", alias = "filepath")]
    path: String,
    #[serde(alias = "target", alias = "old_string", alias = "old_str")]
    find: Option<String>,
    #[serde(
        alias = "replacement",
        alias = "new_string",
        alias = "new_str",
        alias = "with",
        alias = "insert"
    )]
    replace: Option<String>,
    #[serde(
        default,
        deserialize_with = "wxec_core::args::deserialize_line_range_opt",
        alias = "range",
        alias = "line_range",
        alias = "line"
    )]
    lines: Option<String>,
    #[serde(alias = "start", alias = "from")]
    start_line: Option<usize>,
    #[serde(alias = "end", alias = "to")]
    end_line: Option<usize>,
    create: Option<bool>,
    content: Option<String>,
    overwrite: Option<bool>,
    delete: Option<bool>,
    expect_hash: Option<String>,
}

fn fuzzy_match_anchor(old: &str, find: &str) -> Option<(usize, usize)> {
    let find_trimmed_lines: Vec<&str> = find
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if find_trimmed_lines.is_empty() {
        return None;
    }
    let mut old_lines = Vec::new();
    let mut offset = 0;
    for line in old.split_inclusive('\n') {
        let trimmed = line.trim();
        let start = offset;
        let end = offset + line.len();
        offset = end;
        if !trimmed.is_empty() {
            old_lines.push((start, end, trimmed));
        }
    }
    if old_lines.len() < find_trimmed_lines.len() {
        return None;
    }
    let mut matches = Vec::new();
    let n = find_trimmed_lines.len();
    for i in 0..=(old_lines.len() - n) {
        let mut matched = true;
        for j in 0..n {
            if old_lines[i + j].2 != find_trimmed_lines[j] {
                matched = false;
                break;
            }
        }
        if matched {
            let match_start = old_lines[i].0;
            let match_end = old_lines[i + n - 1].1;
            matches.push((match_start, match_end));
            if matches.len() > 1 {
                return None;
            }
        }
    }
    if matches.len() == 1 {
        Some(matches[0])
    } else {
        None
    }
}

impl Tool for EditTool {
    fn name(&self) -> &'static str {
        "edit"
    }
    fn description(&self) -> &'static str {
        "Exact edit, line range or file create."
    }
    fn min_tier(&self) -> Tier {
        Tier::WriteWorkspace
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
                "path": {"type": "string"},
                "find": {"type": "string"},
                "replace": {"type": "string"},
                "lines": {"type": ["string", "array"]},
                "create": {"type": "boolean"},
                "content": {"type": "string"},
                "overwrite": {"type": "boolean", "default": false},
                "delete": {"type": "boolean"},
                "expect_hash": {"type": "string"},
            },
            "required": ["path"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: EditArgs = parse_args(
            args.clone(),
            "edit",
            "use one of find+replace, lines+replace, create+content, delete",
        )?;
        let abs = ctx.resolve(&a.path, false)?;
        let display = ctx.rel(&abs);

        let mut find = a.find.clone();
        if let Some(f) = find.as_deref() {
            if f == "-" {
                let mut s = String::new();
                std::io::stdin()
                    .read_to_string(&mut s)
                    .map_err(|e| WErr::new("io_error", format!("read stdin: {e}")))?;
                find = Some(s);
            } else if let Some(path) = f.strip_prefix('@') {
                let p = ctx.resolve(path, true)?;
                find = Some(
                    std::fs::read_to_string(&p)
                        .map_err(|e| WErr::new("io_error", format!("read @{path}: {e}")))?,
                );
            }
        }

        let mut replace = a.replace.clone();
        if let Some(r) = replace.as_deref() {
            if r == "-" {
                let mut s = String::new();
                std::io::stdin()
                    .read_to_string(&mut s)
                    .map_err(|e| WErr::new("io_error", format!("read stdin: {e}")))?;
                replace = Some(s);
            } else if let Some(path) = r.strip_prefix('@') {
                let p = ctx.resolve(path, true)?;
                replace = Some(
                    std::fs::read_to_string(&p)
                        .map_err(|e| WErr::new("io_error", format!("read @{path}: {e}")))?,
                );
            }
        }

        let mut content = a.content.clone();
        if let Some(c) = content.as_deref() {
            if c == "-" {
                let mut s = String::new();
                std::io::stdin()
                    .read_to_string(&mut s)
                    .map_err(|e| WErr::new("io_error", format!("read stdin: {e}")))?;
                content = Some(s);
            } else if let Some(path) = c.strip_prefix('@') {
                let p = ctx.resolve(path, true)?;
                content = Some(
                    std::fs::read_to_string(&p)
                        .map_err(|e| WErr::new("io_error", format!("read @{path}: {e}")))?,
                );
            }
        }

        let mut lines = a.lines.clone();
        if lines.is_none() && (a.start_line.is_some() || a.end_line.is_some()) {
            let s = a.start_line.unwrap_or(1);
            let e = a.end_line.map(|v| v.to_string()).unwrap_or_default();
            lines = Some(if e.is_empty() {
                format!("{s}:")
            } else {
                format!("{s}:{e}")
            });
        }

        if replace.is_none() && (find.is_some() || lines.is_some()) && content.is_some() {
            replace = content.clone();
        }

        let is_create = a.create.unwrap_or(false)
            || (find.is_none()
                && lines.is_none()
                && !a.delete.unwrap_or(false)
                && content.is_some());

        let modes = [
            find.is_some(),
            lines.is_some(),
            is_create,
            a.delete.unwrap_or(false),
        ];
        match modes.iter().filter(|m| **m).count() {
            1 => {}
            0 => {
                return Err(WErr::invalid_args(
                    "no edit mode",
                    "use find+replace, lines+replace, create+content or delete",
                ))
            }
            _ => {
                return Err(WErr::invalid_args(
                    "multiple edit modes",
                    "use exactly one of find, lines, create, delete",
                ))
            }
        }

        if a.delete.unwrap_or(false) {
            if !abs.exists() {
                return Err(WErr::with_hint(
                    "not_found",
                    format!("'{display}' does not exist"),
                    "check the path",
                ));
            }
            if abs.is_dir() {
                return Err(WErr::with_hint(
                    "is_directory",
                    format!("'{display}' is a directory"),
                    "edit only files",
                ));
            }
            std::fs::remove_file(&abs)
                .map_err(|e| WErr::new("permission_denied", format!("delete {display}: {e}")))?;
            ctx.ledger.invalidate_path(&display);
            ctx.ledger.invalidate_searches();
            ctx.workspace.invalidate();
            let mut r = ToolResult::new("edit", format!("deleted {display}"));
            r.meta.counts.insert("deleted".into(), 1);
            return Ok(r);
        }

        if is_create {
            let content = content.clone().ok_or_else(|| {
                WErr::invalid_args(
                    "create requires content",
                    "content=<file text> or content_file=<path>",
                )
            })?;
            if abs.exists() && a.create == Some(true) && !a.overwrite.unwrap_or(false) {
                return Err(WErr::with_hint(
                    "already_exists",
                    format!("'{display}' already exists"),
                    "pass overwrite=true to replace it",
                ));
            }
            util::atomic_write(&abs, content.as_bytes())?;
            ctx.ledger.invalidate_path(&display);
            ctx.ledger.invalidate_searches();
            ctx.workspace.invalidate();
            let short = hash::short(&hash::hash_str(&content));
            let mut r = ToolResult::new(
                "edit",
                format!("created {display} ({} bytes)", content.len()),
            );
            r.meta.counts.insert("created".into(), 1);
            r.meta.notes.push(format!("hash {short}"));
            post_check(&mut r, ctx, &abs, "", &content)?;
            return Ok(r);
        }

        let file = wxec_index::read_text(&abs)
            .map_err(|e| WErr::new("not_found", format!("read {display}: {e}")))?;
        if file.binary {
            return Err(WErr::with_hint(
                "binary_unsupported",
                format!("'{display}' is binary"),
                "edit text files only",
            ));
        }
        let old = file.content;
        let old_hex = hash::hash_bytes(old.as_bytes());
        let old_short = hash::short(&old_hex);
        if let Some(expect) = &a.expect_hash {
            let exp = expect.trim_start_matches("b3:");
            let ok = !exp.is_empty()
                && (expect == &old_short
                    || expect == &old_hex
                    || old_hex.starts_with(exp)
                    || old_short.trim_start_matches("b3:").starts_with(exp));
            if !ok {
                return Err(WErr::with_hint(
                    "hash_mismatch",
                    format!("current={old_short}"),
                    "re-read the file before editing",
                ));
            }
        }

        let replace = replace.unwrap_or_default();
        let (new, ins, del) = if let Some(find) = &find {
            if find.is_empty() {
                return Err(WErr::invalid_args(
                    "find must not be empty",
                    "pass the exact literal anchor text",
                ));
            }
            if a.replace.is_none() {
                return Err(WErr::invalid_args(
                    "find requires replace",
                    "replace=<new text> (supports - for stdin or @path)",
                ));
            }
            let mut it = old.match_indices(find.as_str()).map(|(i, _)| i);
            let pos_and_len = match it.next() {
                Some(pos) => {
                    if let Some(second) = it.next() {
                        let mut positions = vec![pos, second];
                        let mut more = 0usize;
                        for p in it {
                            more += 1;
                            if positions.len() < 5 {
                                positions.push(p);
                            }
                        }
                        let n = 2 + more;
                        let locs: Vec<String> = positions
                            .iter()
                            .take(5)
                            .map(|p| {
                                let line = old[..*p].matches('\n').count() + 1;
                                let col = p - old[..*p].rfind('\n').map(|i| i + 1).unwrap_or(0) + 1;
                                format!("{line}:{col}")
                            })
                            .collect();
                        return Err(WErr::with_hint(
                            "ambiguous_anchor",
                            format!("{n} matches in {display} at {}", locs.join(", ")),
                            format!("try: w read path={display} lines=<start>:<end>"),
                        ));
                    }
                    Some((pos, find.len()))
                }
                None => {
                    let normalized = if old.contains("\r\n") && !find.contains("\r\n") {
                        find.replace('\n', "\r\n")
                    } else if !old.contains("\r\n") && find.contains("\r\n") {
                        find.replace("\r\n", "\n")
                    } else {
                        find.clone()
                    };
                    if normalized != *find {
                        let mut nit = old.match_indices(normalized.as_str()).map(|(i, _)| i);
                        if let Some(pos) = nit.next() {
                            if nit.next().is_none() {
                                Some((pos, normalized.len()))
                            } else {
                                None
                            }
                        } else if let Some((start, end)) = fuzzy_match_anchor(&old, find) {
                            Some((start, end - start))
                        } else {
                            None
                        }
                    } else if let Some((start, end)) = fuzzy_match_anchor(&old, find) {
                        Some((start, end - start))
                    } else {
                        None
                    }
                }
            };
            let (pos, match_len) = match pos_and_len {
                Some(p) => p,
                None => {
                    let hint = if (find.contains("\r\n") != old.contains("\r\n"))
                        || (find.contains('\t') != old.contains('\t'))
                    {
                        format!("try: anchor may differ in indentation or line endings (CRLF vs LF); check with `w read path={display}`")
                    } else {
                        format!("try: w read path={display} lines=1:80")
                    };
                    return Err(WErr::with_hint(
                        "anchor_not_found",
                        format!("anchor not found in {display}"),
                        hint,
                    ));
                }
            };
            let replace_text = if old.contains("\r\n") && !replace.contains("\r\n") {
                replace.replace('\n', "\r\n")
            } else if !old.contains("\r\n") && replace.contains("\r\n") {
                replace.replace("\r\n", "\n")
            } else {
                replace.clone()
            };
            let mut new = String::with_capacity(old.len() + replace_text.len());
            new.push_str(&old[..pos]);
            new.push_str(&replace_text);
            new.push_str(&old[pos + match_len..]);
            let ins = replace.lines().count().max(1);
            let del = find.lines().count().max(1);
            (new, ins, del)
        } else {
            let spec = lines.as_ref().unwrap();
            let line_count = old.lines().count();
            let (start, end) = parse_line_range(spec, line_count)?;
            let eol = if old.contains("\r\n") { "\r\n" } else { "\n" };
            let had_trailing = old.ends_with('\n');
            let mut lines: Vec<String> = old
                .split('\n')
                .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
                .collect();
            if had_trailing {
                lines.pop();
            }
            let repl: Vec<String> = if replace.is_empty() {
                Vec::new()
            } else {
                let mut rs: Vec<String> = replace
                    .split('\n')
                    .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
                    .collect();
                // strip one trailing empty segment only when replace ends with a line break
                // to avoid inserting an extra blank line on round-trips
                if replace.ends_with('\n') && rs.last().map(|s| s.is_empty()).unwrap_or(false) {
                    rs.pop();
                }
                rs
            };
            let ins = repl.len();
            let del = end - start + 1;
            let mut out_lines: Vec<String> = Vec::new();
            out_lines.extend_from_slice(&lines[..start - 1]);
            out_lines.extend(repl);
            out_lines.extend_from_slice(&lines[end..]);
            let mut new = out_lines.join(eol);
            if had_trailing || replace.ends_with('\n') {
                new.push_str(eol);
            }
            (new, ins, del)
        };

        let bytes = match file.encoding.unwrap_or(wxec_index::TextEncoding::Utf8) {
            enc @ (wxec_index::TextEncoding::Utf16Le
            | wxec_index::TextEncoding::Utf16Be
            | wxec_index::TextEncoding::Latin1) => wxec_index::encode_text(&new, enc)
                .map_err(|e| WErr::new("invalid_args", format!("cannot encode edit: {e}")))?,
            wxec_index::TextEncoding::Utf8 => new.as_bytes().to_vec(),
        };
        util::atomic_write(&abs, &bytes)?;
        ctx.ledger.invalidate_path(&display);
        ctx.ledger.invalidate_searches();
        if let Ok(meta) = std::fs::metadata(&abs) {
            let m = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            ctx.workspace.invalidate_file(&display, meta.len(), m);
        } else {
            ctx.workspace.invalidate();
        }
        let new_hex = hash::hash_bytes(new.as_bytes());
        let short = hash::short(&new_hex);
        let detailed = ctx.config.output.edit_confirm == "detailed";
        let header = if detailed {
            format!("edited {display} (+{ins} -{del}) ({short})")
        } else {
            format!("edited {display} (+{ins} -{del})")
        };
        let mut r = ToolResult::new("edit", header);
        r.meta.counts.insert("inserted".into(), ins as i64);
        r.meta.counts.insert("deleted".into(), del as i64);
        post_check(&mut r, ctx, &abs, &old, &new)?;
        Ok(r)
    }
}

fn post_check(
    r: &mut ToolResult,
    ctx: &mut ToolContext<'_>,
    abs: &Path,
    old_content: &str,
    new_content: &str,
) -> Result<()> {
    if !ctx.config.edit.post_check {
        return Ok(());
    }
    let new_errors = wxec_index::syntax_errors(abs, new_content);
    if new_errors.is_empty() {
        return Ok(());
    }
    let old_errors = if old_content.is_empty() {
        Vec::new()
    } else {
        wxec_index::syntax_errors(abs, old_content)
    };
    let introduced: Vec<_> = new_errors
        .into_iter()
        .filter(|ne| {
            !old_errors
                .iter()
                .any(|oe| oe.message == ne.message && oe.col == ne.col)
        })
        .collect();
    if introduced.is_empty() {
        return Ok(());
    }
    r.header
        .push_str(&format!("; {} syntax errors", introduced.len()));
    r.meta.ok = false;
    for e in &introduced {
        r.diag(Diagnostic {
            file: ctx.rel(abs),
            line: e.line as u32,
            col: Some(e.col as u32),
            severity: Severity::Error,
            code: None,
            message: e.message.clone(),
            tool: Some("tree-sitter".to_string()),
        });
    }
    let mut lines = Vec::new();
    for e in &introduced {
        lines.push(format!(
            "{}:{}:{} error {}",
            ctx.rel(abs),
            e.line,
            e.col,
            e.message
        ));
    }
    let h = ctx
        .handles
        .put("post_check", lines.join("\n").as_bytes(), "text")?;
    for line in lines {
        r.must(line);
    }
    r.handle(HandleRef {
        id: h.id.clone(),
        kind: h.kind.clone(),
        bytes: h.bytes,
        hash: h.hash.clone(),
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// expand
// ---------------------------------------------------------------------------

pub struct ExpandTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpandArgs {
    #[serde(rename = "ref", alias = "id", alias = "handle", alias = "handle_id")]
    reference: String,
    grep: Option<String>,
    #[serde(
        default,
        deserialize_with = "wxec_core::args::deserialize_line_range_opt",
        alias = "range",
        alias = "line_range",
        alias = "line"
    )]
    lines: Option<String>,
    head: Option<u32>,
    tail: Option<u32>,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    all: bool,
    #[serde(default)]
    depth: u32,
}

impl Tool for ExpandTool {
    fn name(&self) -> &'static str {
        "expand"
    }
    fn description(&self) -> &'static str {
        "Expand compacted handle."
    }
    fn min_tier(&self) -> Tier {
        Tier::ReadOnly
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ref": {"type": "string"},
                "grep": {"type": "string"},
                "lines": {"type": "string"},
                "head": {"type": "integer"},
                "tail": {"type": "integer"},
                "offset": {"type": "integer"},
                "all": {"type": "boolean"},
                "depth": {"type": "integer"}
            },
            "required": ["ref"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: ExpandArgs = parse_args(
            args.clone(),
            "expand",
            "pass ref plus optional grep, lines, head, tail, offset",
        )?;
        let clean_ref = a.reference.trim_start_matches('~');
        let id = format!("~{clean_ref}");
        let (bytes, rec) = ctx.handles.get(&id)?;
        if rec.content_type == "bytes" {
            let mut r = ToolResult::new(
                "expand",
                format!(
                    "{id} binary {} bytes ({})",
                    rec.bytes,
                    hash::short(&rec.hash)
                ),
            );
            r.meta.counts.insert("bytes".into(), rec.bytes as i64);
            return Ok(r);
        }
        let text_cow = String::from_utf8_lossy(&bytes);
        let all: Vec<&str> = text_cow.lines().collect();
        let total = all.len();
        let mut selected: Vec<&str> = Vec::new();
        if let Some(pattern) = &a.grep {
            let re = regex::Regex::new(pattern).map_err(|e| {
                WErr::invalid_args(
                    format!("invalid regex '{pattern}': {e}"),
                    "check escaping or use a simpler pattern",
                )
            })?;
            if let Some(spec) = &a.lines {
                let (start, end) = parse_line_range_clamped(spec, total.max(1))?;
                for (idx, line) in all.iter().enumerate() {
                    let n = idx + 1;
                    if n >= start && n <= end && re.is_match(line) {
                        selected.push(*line);
                    }
                }
            } else {
                for line in &all {
                    if re.is_match(line) {
                        selected.push(*line);
                    }
                }
            }
        } else if let Some(spec) = &a.lines {
            let (start, end) = parse_line_range_clamped(spec, total.max(1))?;
            for (idx, line) in all.iter().enumerate() {
                let n = idx + 1;
                if n >= start && n <= end {
                    selected.push(*line);
                }
            }
        } else {
            selected = all;
        }
        if let Some(head) = a.head {
            selected.truncate(head as usize);
        }
        if let Some(tail) = a.tail {
            let skip = selected.len().saturating_sub(tail as usize);
            selected = selected.split_off(skip);
        }
        let filtered_total = selected.len();
        let offset = a.offset as usize;
        if offset > 0 {
            if offset >= selected.len() {
                selected.clear();
            } else {
                selected = selected.split_off(offset);
            }
        }

        let arg_filtered = a.grep.is_some()
            || a.lines.is_some()
            || a.head.is_some()
            || a.tail.is_some()
            || a.offset > 0;
        if !a.all {
            let cap = ctx.config.handles.expand_max_tokens.max(64) as usize;
            let mut used = 0usize;
            let mut page_len = selected.len();
            for (i, line) in selected.iter().enumerate() {
                let t = wxec_core::estimator::estimate_text(line).max(1) as usize;
                if i > 0 && used.saturating_add(t) > cap {
                    page_len = i;
                    break;
                }
                used = used.saturating_add(t);
            }
            if page_len < selected.len() {
                selected.truncate(page_len);
            }
        }
        let consumed = offset + selected.len();
        let remaining = filtered_total.saturating_sub(consumed);
        let page_truncated = !a.all && remaining > 0;
        let show_count = arg_filtered || page_truncated;
        let mut r = ToolResult::new(
            "expand",
            if show_count {
                format!(
                    "{id} {} of {total} lines ({} bytes, {})",
                    selected.len(),
                    rec.bytes,
                    rec.kind
                )
            } else {
                format!("{id} {total} lines ({} bytes, {})", rec.bytes, rec.kind)
            },
        );
        fn inline_nested(
            text: &str,
            ctx: &mut ToolContext<'_>,
            depth: u32,
            budget: &mut usize,
        ) -> String {
            if depth == 0 || *budget == 0 {
                return text.to_string();
            }
            let re = regex::Regex::new(r"\(~([A-Za-z0-9]+)\)").ok();
            let Some(re) = re else {
                return text.to_string();
            };
            let mut out = String::with_capacity(text.len());
            let mut last = 0usize;
            for cap in re.captures_iter(text) {
                let m = cap.get(0).unwrap();
                let idm = cap.get(1).unwrap();
                out.push_str(&text[last..m.start()]);
                let id = format!("~{}", idm.as_str());
                if *budget == 0 {
                    out.push_str(m.as_str());
                    last = m.end();
                    continue;
                }
                if let Ok((b, rec)) = ctx.handles.get(&id) {
                    if rec.content_type == "text" && b.len() <= 20000 && *budget >= b.len() {
                        let inner = String::from_utf8_lossy(&b).into_owned();
                        *budget = budget.saturating_sub(b.len());
                        let inlined = inline_nested(&inner, ctx, depth - 1, budget);
                        out.push_str("\n# inline ");
                        out.push_str(&id);
                        out.push('\n');
                        out.push_str(inlined.trim_end());
                        out.push('\n');
                    } else {
                        out.push_str(m.as_str());
                    }
                } else {
                    out.push_str(m.as_str());
                }
                last = m.end();
            }
            out.push_str(&text[last..]);
            out
        }

        let mut body = selected.join("\n") + if selected.is_empty() { "" } else { "\n" };
        if a.all {
            let mut depth = a.depth;
            if depth == 0 {
                depth = 8;
            }
            let mut budget = 8 * 1024 * 1024usize;
            body = inline_nested(&body, ctx, depth.min(64), &mut budget);
            r.raw_text = Some(body.clone());
            for line in body.lines().take(2000) {
                r.low(line.to_string());
            }
            if body.lines().count() > 2000 {
                let h = ctx.handles.put("expand_all", body.as_bytes(), "text")?;
                r.must(format!(
                    "...+{} lines ({})",
                    body.lines().count().saturating_sub(2000),
                    h.id
                ));
                r.handle(HandleRef {
                    id: h.id.clone(),
                    kind: h.kind.clone(),
                    bytes: h.bytes,
                    hash: h.hash.clone(),
                });
            }
        } else {
            for line in &selected {
                r.must((*line).to_string());
            }
        }
        if remaining > 0 && !a.all {
            r.note(format!("remaining={remaining} next_offset={consumed}"));
        }
        r.meta.counts.insert("lines".into(), selected.len() as i64);
        r.meta.counts.insert("bytes".into(), rec.bytes as i64);
        Ok(r)
    }
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(TreeTool));
    out.push(std::sync::Arc::new(ReadTool));
    out.push(std::sync::Arc::new(EditTool));
    out.push(std::sync::Arc::new(ExpandTool));
}

pub fn root_abs(ctx: &ToolContext<'_>, path: &str) -> PathBuf {
    util::path_buf(ctx.root, path)
}

pub fn _unused(_: &Workspace) {}
