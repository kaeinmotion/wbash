use std::sync::Arc;

use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};

use wxec_core::args::parse_args;
use wxec_core::error::{Result, WErr};
use wxec_core::registry::{Tool, ToolContext};
use wxec_core::types::{HandleRef, Tier, ToolResult};

use crate::util;

pub struct ReplaceTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplaceArgs {
    #[serde(
        alias = "target",
        alias = "old_string",
        alias = "old_str",
        alias = "pattern",
        alias = "search",
        alias = "query"
    )]
    find: String,
    #[serde(
        default,
        alias = "replacement",
        alias = "new_string",
        alias = "new_str",
        alias = "with",
        alias = "to",
        alias = "insert"
    )]
    replace: String,
    #[serde(
        default = "dot",
        alias = "file",
        alias = "file_path",
        alias = "filepath",
        alias = "dir",
        alias = "directory"
    )]
    path: String,
    #[serde(default, alias = "files")]
    paths: Vec<String>,
    glob: Option<String>,
    #[serde(default)]
    word: bool,
    #[serde(default)]
    regex: bool,
    #[serde(default, alias = "preview")]
    dry_run: bool,
    #[serde(
        default = "max_files_default",
        deserialize_with = "wxec_core::args::deserialize_u32_lenient"
    )]
    max_files: u32,
    #[serde(
        default,
        deserialize_with = "wxec_core::args::deserialize_u32_opt_lenient"
    )]
    limit: Option<u32>,
}

fn dot() -> String {
    ".".to_string()
}
fn max_files_default() -> u32 {
    200
}

enum Matcher {
    Literal(String),
    Word(Regex),
    Regex(Regex),
}

impl Matcher {
    fn count(&self, text: &str) -> usize {
        match self {
            Matcher::Literal(f) => text.match_indices(f).count(),
            Matcher::Word(re) | Matcher::Regex(re) => re.find_iter(text).count(),
        }
    }
    fn replace_all(&self, text: &str, repl: &str, limit: Option<usize>) -> (String, usize) {
        match self {
            Matcher::Literal(f) => replace_literal(text, f, repl, limit),
            Matcher::Word(re) | Matcher::Regex(re) => {
                let mut n = 0usize;
                let out = re
                    .replace_all(text, |caps: &regex::Captures<'_>| {
                        if limit.is_none_or(|lim| n < lim) {
                            n += 1;
                            expand_replacement(repl, caps)
                        } else {
                            caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string()
                        }
                    })
                    .into_owned();
                (out, n)
            }
        }
    }
}

fn replace_literal(text: &str, find: &str, repl: &str, limit: Option<usize>) -> (String, usize) {
    let mut out = String::with_capacity(text.len());
    let mut last = 0usize;
    let mut n = 0usize;
    for (i, _) in text.match_indices(find) {
        if limit.map(|l| n >= l).unwrap_or(false) {
            break;
        }
        out.push_str(&text[last..i]);
        out.push_str(repl);
        last = i + find.len();
        n += 1;
    }
    out.push_str(&text[last..]);
    (out, n)
}

fn expand_replacement(template: &str, caps: &regex::Captures<'_>) -> String {
    // simple $0/$1.. expansion; good enough for agent use
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            if chars.peek() == Some(&'$') {
                chars.next();
                out.push('$');
                continue;
            }
            if chars.peek() == Some(&'0') {
                chars.next();
                if let Some(m) = caps.get(0) {
                    out.push_str(m.as_str());
                }
                continue;
            }
            let mut num = String::new();
            while let Some(d) = chars.peek().copied() {
                if d.is_ascii_digit() {
                    num.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            if !num.is_empty() {
                if let Ok(i) = num.parse::<usize>() {
                    if let Some(m) = caps.get(i) {
                        out.push_str(m.as_str());
                    }
                } else {
                    out.push('$');
                    out.push_str(&num);
                }
                continue;
            }
            out.push('$');
        } else {
            out.push(c);
        }
    }
    out
}

impl Tool for ReplaceTool {
    fn name(&self) -> &'static str {
        "replace"
    }
    fn description(&self) -> &'static str {
        "Replace matches across files."
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
                "find": {"type": "string"},
                "replace": {"type": "string"},
                "path": {"type": "string"},
                "paths": {"type": "array"},
                "glob": {"type": "string"},
                "word": {"type": "boolean"},
                "regex": {"type": "boolean"},
                "dry_run": {"type": "boolean"},
                "max_files": {"type": "integer"},
                "limit": {"type": "integer"}
            },
            "required": ["find"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: ReplaceArgs = parse_args(
            args.clone(),
            "replace",
            "pass find and replace plus optional path/paths/glob/word/regex/dry_run/max_files/limit",
        )?;
        let find_str = crate::fs::resolve_content_or_handle(ctx, &a.find)?;
        let repl_str = crate::fs::resolve_content_or_handle(ctx, &a.replace)?;
        if find_str.is_empty() {
            return Err(WErr::invalid_args(
                "find must not be empty",
                "pass the text or pattern to replace",
            ));
        }
        if a.word && a.regex {
            return Err(WErr::invalid_args(
                "word and regex cannot both be true",
                "use word for literal word matches or regex for patterns",
            ));
        }
        let matcher = if a.regex {
            Matcher::Regex(Regex::new(&find_str).map_err(|e| {
                WErr::invalid_args(
                    format!("invalid regex '{find_str}': {e}"),
                    "check escaping or use literal mode",
                )
            })?)
        } else if a.word {
            let pat = format!(r"\b{}\b", regex::escape(&find_str));
            Matcher::Word(Regex::new(&pat).map_err(|e| {
                WErr::invalid_args(
                    format!("invalid word pattern: {e}"),
                    "check the find string",
                )
            })?)
        } else {
            Matcher::Literal(find_str)
        };

        let mut bases_rel = Vec::new();
        if a.paths.is_empty() {
            let base = crate::fs::resolve_workspace_path(ctx, &a.path)?;
            bases_rel.push(ctx.rel(&base));
        } else {
            for p in &a.paths {
                let base = crate::fs::resolve_workspace_path(ctx, p)?;
                bases_rel.push(ctx.rel(&base));
            }
        }
        let gs = util::globset(a.glob.as_deref())?;
        let walk = ctx.workspace.walk_cached_arc();

        let mut candidates: Vec<&wxec_index::FileEntry> = Vec::new();
        for f in walk.files.iter() {
            let in_scope = bases_rel.iter().any(|b| util::under(&f.rel, b))
                && util::matches_glob(gs.as_ref(), &f.rel);
            if in_scope {
                candidates.push(f);
            }
        }
        if candidates.len() > (a.max_files as usize).saturating_mul(20) {
            return Err(WErr::with_hint(
                "too_many_files",
                format!(
                    "{} files in scope (max scan {})",
                    candidates.len(),
                    a.max_files
                ),
                "narrow path/glob",
            ));
        }

        struct Planned {
            rel: String,
            abs: std::path::PathBuf,
            bytes: Vec<u8>,
        }

        let verbose = ctx.config.output.verbosity == "verbose";
        const MAX_DIFF_LINES: usize = 2000;
        let mut planned: Vec<Planned> = Vec::new();
        let mut total_replacements = 0usize;
        let mut files_with_matches = 0usize;
        let mut remaining_limit = a.limit.map(|l| l as usize);
        let mut file_summaries: Vec<String> = Vec::new();
        let mut full_diff: Vec<String> = Vec::new();

        for f in candidates {
            if remaining_limit == Some(0) {
                break;
            }
            if wxec_index::is_known_binary_ext(&f.rel) || f.size > 2_000_000 {
                continue;
            }
            let Ok(tf) = ctx.workspace.read_cached_text(f) else {
                continue;
            };
            if tf.binary {
                continue;
            }
            let old = tf.content;
            let n = matcher.count(&old);
            if n == 0 {
                continue;
            }
            files_with_matches += 1;
            if files_with_matches > a.max_files as usize {
                return Err(WErr::with_hint(
                    "too_many_files",
                    format!(
                        "{} files with matches (max {})",
                        files_with_matches, a.max_files
                    ),
                    "narrow path/glob or raise max_files",
                ));
            }
            let apply_n = remaining_limit.map(|l| l.min(n)).unwrap_or(n);
            let (new, replaced) = matcher.replace_all(&old, &repl_str, Some(apply_n));
            if replaced == 0 || new == old {
                continue;
            }
            total_replacements += replaced;
            if let Some(ref mut lim) = remaining_limit {
                *lim = lim.saturating_sub(replaced);
            }

            if verbose && full_diff.len() < MAX_DIFF_LINES {
                for l in render_file_diff(&f.rel, &old, &new) {
                    if full_diff.len() >= MAX_DIFF_LINES {
                        break;
                    }
                    full_diff.push(l);
                }
            }
            let bytes = match tf.encoding.unwrap_or(wxec_index::TextEncoding::Utf8) {
                enc @ (wxec_index::TextEncoding::Utf16Le
                | wxec_index::TextEncoding::Utf16Be
                | wxec_index::TextEncoding::Latin1) => {
                    wxec_index::encode_text(&new, enc).map_err(|e| {
                        WErr::new("invalid_args", format!("cannot encode replace: {e}"))
                    })?
                }
                wxec_index::TextEncoding::Utf8 => new.as_bytes().to_vec(),
            };
            file_summaries.push(format!("{} ({})", f.rel, replaced));
            planned.push(Planned {
                rel: f.rel.clone(),
                abs: f.abs.clone(),
                bytes,
            });
        }

        if total_replacements == 0 {
            let msg = if let Some(ig) = bases_rel.iter().find_map(|b| util::ignored_component(b)) {
                format!("0 matches (path is under built-in ignored directory '{ig}')")
            } else {
                "0 matches".to_string()
            };
            return Err(WErr::with_hint(
                "replace_not_found",
                msg,
                "check find/glob/path; use word=false or regex for patterns",
            ));
        }

        let files_changed = planned.len();

        if !a.dry_run {
            for p in &planned {
                util::atomic_write(&p.abs, &p.bytes)?;
                ctx.ledger.invalidate_path(&p.rel);
                if let Ok(meta) = std::fs::metadata(&p.abs) {
                    let m = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    ctx.workspace.invalidate_file(&p.rel, meta.len(), m);
                }
            }
            ctx.ledger.invalidate_searches();
        }

        let header = if a.dry_run {
            format!(
                "dry_run: {} in {}",
                util::plural(total_replacements, "match"),
                util::plural(files_changed, "file")
            )
        } else if files_changed == 1 {
            format!(
                "replaced {} in {}",
                util::plural(total_replacements, "match"),
                planned[0].rel
            )
        } else {
            format!(
                "replaced {} in {}",
                util::plural(total_replacements, "match"),
                util::plural(files_changed, "file")
            )
        };
        let mut result = ToolResult::new("replace", header);
        if verbose {
            let truncated = full_diff.len() >= MAX_DIFF_LINES;
            for l in full_diff {
                result.low(l);
            }
            if truncated {
                result.note("diff_truncated");
            }
        } else if files_changed == 1 {
            // single-file replacement already named in header
        } else if files_changed <= 6 {
            for s in file_summaries {
                result.low(s);
            }
        } else {
            for s in file_summaries.iter().take(6).cloned() {
                result.low(s);
            }
            let omitted: Vec<String> = file_summaries.iter().skip(6).cloned().collect();
            let h = ctx
                .handles
                .put("replace", omitted.join("\n").as_bytes(), "text")?;
            result.low(format!("...+{} files ({})", omitted.len(), h.id));
            result.handle(HandleRef {
                id: h.id.clone(),
                kind: h.kind.clone(),
                bytes: h.bytes,
                hash: h.hash.clone(),
            });
        }
        result
            .meta
            .counts
            .insert("replacements".into(), total_replacements as i64);
        result
            .meta
            .counts
            .insert("files".into(), files_changed as i64);
        if a.dry_run {
            result.note("dry_run");
        }
        Ok(result)
    }
}

fn render_file_diff(path: &str, old: &str, new: &str) -> Vec<String> {
    let diff = TextDiff::from_lines(old, new);
    let mut out = Vec::new();
    let mut old_line = 0usize;
    let mut new_line = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                old_line += 1;
                new_line += 1;
            }
            ChangeTag::Delete => {
                old_line += 1;
                out.push(format!(
                    "{}:{}: -{}",
                    path,
                    old_line,
                    change.value().trim_end_matches(['\n', '\r'])
                ));
            }
            ChangeTag::Insert => {
                new_line += 1;
                out.push(format!(
                    "{}:{}: +{}",
                    path,
                    new_line,
                    change.value().trim_end_matches(['\n', '\r'])
                ));
            }
        }
    }
    out
}

pub fn register(out: &mut Vec<Arc<dyn Tool>>) {
    out.push(Arc::new(ReplaceTool));
}
