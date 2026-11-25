use std::collections::HashMap;

use serde::Deserialize;
use serde_json::{json, Value};

use wxec_core::args::parse_args;
use wxec_core::error::{Result, WErr};
use wxec_core::registry::{Tool, ToolContext};
use wxec_core::types::{now_ms, HandleRef, Item, ItemClass, SearchSeen, Tier, ToolResult};

use crate::util;

pub struct SearchTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArgs {
    #[serde(alias = "query", alias = "q", alias = "term", alias = "text")]
    pattern: String,
    #[serde(default = "text")]
    mode: String,
    #[serde(default = "dot", alias = "dir", alias = "directory", alias = "folder")]
    path: String,
    #[serde(default, alias = "files")]
    paths: Vec<String>,
    glob: Option<String>,
    #[serde(default, deserialize_with = "wxec_core::args::deserialize_u32_lenient")]
    context: u32,
    #[serde(
        default,
        deserialize_with = "wxec_core::args::deserialize_u32_opt_lenient"
    )]
    limit: Option<u32>,
    #[serde(
        default,
        alias = "files_only",
        alias = "names_only",
        alias = "name_only",
        alias = "only_files"
    )]
    files_only: bool,
    #[serde(default)]
    stats: bool,
    #[serde(default, alias = "ignore_case", alias = "case_insensitive")]
    case_insensitive: bool,
    #[serde(default)]
    case_sensitive: Option<bool>,
    #[serde(default, alias = "whole_word")]
    word: bool,
}

fn text() -> String {
    "text".to_string()
}
fn dot() -> String {
    ".".to_string()
}

#[derive(Debug, Clone)]
struct Hit {
    path: String,
    line: u32,
    col: u32,
    snippet: String,
    is_definition: bool,
    score: i32,
}

#[derive(Debug, Clone, Copy)]
enum MatchMode<'a> {
    Literal(&'a str),
    Regex(&'a regex::Regex),
}

struct ScanStats {
    files_with_hits: usize,
    fallback: bool,
    scanned_files: usize,
    skipped_binary: usize,
}

impl Tool for SearchTool {
    fn name(&self) -> &'static str {
        "search"
    }
    fn description(&self) -> &'static str {
        "Search workspace."
    }
    fn min_tier(&self) -> Tier {
        Tier::ReadOnly
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "mode": {"type": "string", "enum": ["text", "regex", "symbol"]},
                "path": {"type": "string"},
                "paths": {"type": "array"},
                "glob": {"type": "string"},
                "context": {"type": "integer"},
                "limit": {"type": "integer"},
                "files_only": {"type": "boolean"},
                "stats": {"type": "boolean"},
                "case_insensitive": {"type": "boolean"}
            },
            "required": ["pattern"],
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: SearchArgs = parse_args(
            args.clone(),
            "search",
            "pass pattern plus optional mode, path, glob, context, limit",
        )?;
        if a.pattern.is_empty() {
            return Err(WErr::invalid_args(
                "pattern must not be empty",
                "pattern=<text>",
            ));
        }
        if a.context > ctx.config.search.context_max {
            return Err(WErr::invalid_args(
                format!(
                    "context {} exceeds max {}",
                    a.context, ctx.config.search.context_max
                ),
                format!("context must be 0..={}", ctx.config.search.context_max),
            ));
        }
        let limit = a.limit.unwrap_or(ctx.config.search.default_limit);
        if limit == 0 || limit > ctx.config.search.max_limit {
            return Err(WErr::invalid_args(
                format!("limit {limit} out of range"),
                format!("limit must be 1..={}", ctx.config.search.max_limit),
            ));
        }
        let mut bases_rel = Vec::new();
        if a.paths.is_empty() {
            let base = ctx.resolve(&a.path, true)?;
            bases_rel.push(ctx.rel(&base));
        } else {
            for p in &a.paths {
                let base = ctx.resolve(p, true)?;
                bases_rel.push(ctx.rel(&base));
            }
        }
        let mut cache_bases = bases_rel.clone();
        cache_bases.sort();
        let cache_key = format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
            a.pattern,
            a.mode,
            cache_bases.join("\0"),
            a.glob.clone().unwrap_or_default(),
            a.context,
            limit,
            a.stats,
            a.files_only
        );
        if let Some(seen) = ctx
            .ledger
            .search_entry(&cache_key, ctx.config.session.ledger_ttl_secs, now_ms())
            .cloned()
        {
            let snap_id = seen
                .snapshot_ref
                .as_deref()
                .filter(|id| ctx.handles.has_fresh(id))
                .map(|s| s.to_string());
            let header = if let Some(id) = snap_id {
                format!(
                    "unchanged search ({}, {}) expand {}",
                    util::plural(seen.files, "file"),
                    util::plural(seen.matches, "match"),
                    id
                )
            } else {
                format!(
                    "unchanged search ({}, {})",
                    util::plural(seen.files, "file"),
                    util::plural(seen.matches, "match")
                )
            };
            let mut r = ToolResult::new("search", header);
            r.meta.counts.insert("files".into(), seen.files as i64);
            r.meta.counts.insert("matches".into(), seen.matches as i64);
            return Ok(r);
        }
        let gs = util::globset(a.glob.as_deref())?;
        let is_case_insensitive = a.case_insensitive || a.case_sensitive == Some(false);
        let regex = if a.mode == "regex" || a.word || is_case_insensitive {
            let pat = if a.mode == "regex" {
                if a.word {
                    format!(r"\b(?:{})\b", a.pattern)
                } else {
                    a.pattern.clone()
                }
            } else if a.word {
                format!(r"\b{}\b", regex::escape(&a.pattern))
            } else {
                regex::escape(&a.pattern)
            };
            Some(
                regex::RegexBuilder::new(&pat)
                    .case_insensitive(is_case_insensitive)
                    .build()
                    .map_err(|e| {
                        WErr::invalid_args(
                            format!("invalid pattern '{}': {e}", a.pattern),
                            "check escaping or use mode=text",
                        )
                    })?,
            )
        } else {
            None
        };

        let mut hits: Vec<Hit> = Vec::new();
        let mut stats = ScanStats {
            files_with_hits: 0,
            fallback: false,
            scanned_files: 0,
            skipped_binary: 0,
        };
        let pat_lc = a.pattern.to_ascii_lowercase();
        let pat_near = format!(" {pat_lc} ");

        let walk = ctx.workspace.walk_cached_arc();
        let filter = |rel: &str| {
            bases_rel.iter().any(|b| util::under(rel, b)) && util::matches_glob(gs.as_ref(), rel)
        };

        if a.mode == "symbol" {
            scan_symbol(ctx, &a, &walk, &filter, &pat_lc, &mut hits, &mut stats);
        } else {
            let m = match &regex {
                Some(re) => MatchMode::Regex(re),
                None => MatchMode::Literal(&a.pattern),
            };
            scan_lines(
                ctx, &walk, &filter, &a, m, &pat_lc, &pat_near, &mut hits, &mut stats,
            );
        }

        let mut alt_matched = false;
        if a.mode == "text" && hits.is_empty() {
            if let Some(alt_pat) = safe_alternation(&a.pattern) {
                if let Ok(alt_re) = regex::Regex::new(&alt_pat) {
                    hits.clear();
                    stats = ScanStats {
                        files_with_hits: 0,
                        fallback: false,
                        scanned_files: 0,
                        skipped_binary: 0,
                    };
                    scan_lines(
                        ctx,
                        &walk,
                        &filter,
                        &a,
                        MatchMode::Regex(&alt_re),
                        &pat_lc,
                        &pat_near,
                        &mut hits,
                        &mut stats,
                    );
                    if !hits.is_empty() {
                        alt_matched = true;
                    }
                }
            }
        }

        if a.files_only {
            let all_files: Vec<String> = if a.mode == "symbol" {
                let file_set: std::collections::BTreeSet<String> =
                    hits.into_iter().map(|h| h.path).collect();
                file_set.into_iter().collect()
            } else {
                // walk.files is already sorted deterministically and scan_lines pushes at most once per file
                hits.into_iter().map(|h| h.path).collect()
            };
            let file_count = all_files.len();
            let file_limit = a.limit.unwrap_or(200) as usize;
            let truncated_files = file_count > file_limit;
            let returned_files = file_count.min(file_limit);
            let mut result = ToolResult::new(
                "search",
                if truncated_files {
                    format!(
                        "{} with matches ({returned_files} returned)",
                        util::plural(file_count, "file")
                    )
                } else {
                    format!("{} with matches", util::plural(file_count, "file"))
                },
            );
            result.push(Item::new(
                ItemClass::High,
                5.0,
                all_files[..returned_files].to_vec(),
            ));
            if truncated_files {
                let omitted = all_files[returned_files..].join("\n");
                let h = ctx.handles.put("search", omitted.as_bytes(), "text")?;
                result.must(format!(
                    "...+{} files ({})",
                    file_count - returned_files,
                    h.id
                ));
                result.handle(HandleRef {
                    id: h.id,
                    kind: h.kind,
                    bytes: h.bytes,
                    hash: h.hash,
                });
            }
            result.meta.counts.insert("files".into(), file_count as i64);
            return Ok(result);
        }
        hits.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(b.is_definition.cmp(&a.is_definition))
                .then(a.path.cmp(&b.path))
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
        });
        let total = hits.len();
        let truncated = total > limit as usize;
        let returned = hits.len().min(limit as usize);
        let omitted_body = if truncated {
            let mut body = String::new();
            for h in hits.iter().skip(returned) {
                use std::fmt::Write as _;
                let _ = writeln!(body, "{}:{}: {}", h.path, h.line, h.snippet);
            }
            if body.ends_with('\n') {
                body.pop();
            }
            body
        } else {
            String::new()
        };
        hits.truncate(returned);

        let mut result = ToolResult::new(
            "search",
            if truncated {
                format!(
                    "{}, {} ({returned} returned)",
                    util::plural(stats.files_with_hits, "file"),
                    util::plural(total, "match")
                )
            } else {
                format!(
                    "{}, {}",
                    util::plural(stats.files_with_hits, "file"),
                    util::plural(total, "match")
                )
            },
        );
        result
            .meta
            .counts
            .insert("files".into(), stats.files_with_hits as i64);
        result.meta.counts.insert("matches".into(), total as i64);
        result
            .meta
            .counts
            .insert("returned".into(), returned as i64);
        result
            .meta
            .counts
            .insert("scanned".into(), stats.scanned_files as i64);
        ctx.ledger.record_search(
            cache_key,
            SearchSeen {
                snapshot_ref: None,
                files: stats.files_with_hits,
                matches: total,
                shown_ms: now_ms(),
            },
        );
        if total == 1
            && !truncated
            && !alt_matched
            && !stats.fallback
            && !walk.truncated
            && stats.skipped_binary == 0
            && a.context == 0
        {
            if let Some(h) = hits.first() {
                result.header = format!("{}:{}: {}", h.path, h.line, h.snippet);
                result.items.clear();
                return Ok(result);
            }
        }
        if alt_matched {
            result.note("'|' alternation");
        } else if a.mode == "text" && total == 0 && a.pattern.contains('|') {
            result.note("'|' literal; try mode=regex");
        }
        if a.mode == "symbol" && stats.fallback {
            result.note("symbol_index_unavailable fallback_scan");
        }
        if walk.truncated {
            result.note("search_truncated");
        }
        if total == 0 && stats.skipped_binary > 0 && ctx.config.output.verbosity == "verbose" {
            result.note(format!(
                "skipped {}",
                util::plural(stats.skipped_binary, "binary file")
            ));
        }

        let mut context_lines: Vec<String> = Vec::new();
        let mut context_text_cache: HashMap<String, std::sync::Arc<str>> = HashMap::new();

        let mut file_order: Vec<String> = Vec::new();
        let mut by_file: HashMap<String, Vec<Hit>> = HashMap::new();
        for h in hits.drain(..) {
            let e = by_file.entry(h.path.clone()).or_insert_with(|| {
                file_order.push(h.path.clone());
                Vec::new()
            });
            e.push(h);
        }

        for path in &file_order {
            let Some(mut fhits) = by_file.remove(path) else {
                continue;
            };
            fhits.sort_by(|a, b| a.line.cmp(&b.line).then(a.col.cmp(&b.col)));
            let mut seen_ctx_lines: std::collections::HashSet<u32> =
                fhits.iter().map(|h| h.line).collect();
            let grouped = fhits.len() >= 2;
            if grouped {
                result.push(Item::new(
                    ItemClass::High,
                    fhits
                        .iter()
                        .map(|h| if h.is_definition { 8.0 } else { 3.0 })
                        .fold(0.0f32, f32::max),
                    vec![format!("{} ({}):", path, fhits.len())],
                ));
            }
            let mut i = 0usize;
            while i < fhits.len() {
                let hit = &fhits[i];
                let class = if hit.is_definition {
                    ItemClass::High
                } else {
                    ItemClass::Low
                };
                let mut lines = vec![hit.line];
                let mut j = i + 1;
                while j < fhits.len()
                    && fhits[j].snippet == hit.snippet
                    && fhits[j].is_definition == hit.is_definition
                {
                    if a.context == 0 {
                        lines.push(fhits[j].line);
                        j += 1;
                    } else {
                        break;
                    }
                }
                let loc = format_line_numbers(&lines);
                let line = if grouped {
                    format!("  {}: {}", loc, hit.snippet)
                } else {
                    format!("{}:{}: {}", hit.path, loc, hit.snippet)
                };
                if grouped {
                    result.low(line);
                } else {
                    result.push(Item::new(
                        class,
                        if hit.is_definition { 8.0 } else { 3.0 },
                        vec![line],
                    ));
                }
                if a.context > 0 {
                    let text = context_text_cache
                        .entry(hit.path.clone())
                        .or_insert_with(|| {
                            ctx.workspace
                                .read_search_text_for_path(&hit.path)
                                .unwrap_or_else(|_| std::sync::Arc::from(""))
                        });
                    if !text.is_empty() {
                        let lo = (hit.line as usize).saturating_sub(a.context as usize + 1);
                        let hi = (hit.line as usize) + a.context as usize;
                        for (k, l) in text.lines().enumerate().take(hi).skip(lo) {
                            let n = (k + 1) as u32;
                            if seen_ctx_lines.insert(n) {
                                context_lines.push(format!("{}:{}- {}", hit.path, n, l.trim()));
                            }
                        }
                    }
                }
                i = j;
            }
        }
        for line in context_lines {
            result.low(line);
        }

        if truncated {
            let omitted = total - returned;
            let h = ctx.handles.put("search", omitted_body.as_bytes(), "text")?;
            result.must(format!("...+{omitted} more ({})", h.id));
            result.handle(HandleRef {
                id: h.id.clone(),
                kind: h.kind.clone(),
                bytes: h.bytes,
                hash: h.hash.clone(),
            });
        }

        if a.stats {
            result.must(format!(
                "# stats: scanned {} files, {} matches",
                stats.scanned_files, total
            ));
        }

        Ok(result)
    }
}

fn format_line_numbers(lines: &[u32]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    if lines.len() == 1 {
        return lines[0].to_string();
    }
    let mut ranges = Vec::new();
    let mut start = lines[0];
    let mut prev = lines[0];
    for &cur in &lines[1..] {
        if cur == prev + 1 {
            prev = cur;
        } else {
            if start == prev {
                ranges.push(format!("{start}"));
            } else if prev == start + 1 {
                ranges.push(format!("{start},{prev}"));
            } else {
                ranges.push(format!("{start}-{prev}"));
            }
            start = cur;
            prev = cur;
        }
    }
    if start == prev {
        ranges.push(format!("{start}"));
    } else if prev == start + 1 {
        ranges.push(format!("{start},{prev}"));
    } else {
        ranges.push(format!("{start}-{prev}"));
    }
    ranges.join(",")
}

fn safe_alternation(pattern: &str) -> Option<String> {
    if pattern.contains("||") {
        return None;
    }
    let parts: Vec<&str> = pattern.split('|').collect();
    if parts.len() < 2 || parts.len() > 16 {
        return None;
    }
    let mut branches = Vec::with_capacity(parts.len());
    for p in parts {
        let t = p.trim();
        if t.is_empty() || t.len() != p.len() {
            return None;
        }
        if t.chars().any(|c| c.is_whitespace()) {
            return None;
        }
        if !t.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '@' | ':' | '+' | '-')
        }) {
            return None;
        }
        branches.push(regex::escape(t));
    }
    Some(format!("(?:{})", branches.join("|")))
}

fn scan_symbol(
    ctx: &mut ToolContext<'_>,
    a: &SearchArgs,
    walk: &wxec_index::WalkResult,
    filter: &impl Fn(&str) -> bool,
    pat_lc: &str,
    hits: &mut Vec<Hit>,
    stats: &mut ScanStats,
) {
    for f in walk.files.iter() {
        if !filter(&f.rel) {
            continue;
        }
        if wxec_index::is_known_binary_ext(&f.rel) || f.size > 2_000_000 {
            stats.skipped_binary += 1;
            continue;
        }
        stats.scanned_files += 1;
        let Ok((text, binary)) = ctx.workspace.read_search_text(f) else {
            continue;
        };
        if binary {
            stats.skipped_binary += 1;
            continue;
        }
        let path_lc = f.rel.to_ascii_lowercase();
        let outline = ctx
            .symbols
            .symbols_cached(&f.abs, &text, f.mtime_ms, f.size);
        if outline.heuristic {
            stats.fallback = true;
        }
        let before = hits.len();
        for s in outline.symbols {
            let name_lc = s.name.to_ascii_lowercase();
            if s.name == a.pattern || name_lc.contains(pat_lc) {
                let mut score = 10;
                if s.name == a.pattern {
                    score += 8;
                } else if name_lc == pat_lc {
                    score += 6;
                }
                score += path_boost_lc(&path_lc, pat_lc);
                let line_text = s.line_text.trim();
                let kind_lc = s.kind.to_ascii_lowercase();
                let lt_lc = line_text.to_ascii_lowercase();
                let raw_snippet = if lt_lc.starts_with(&format!("{kind_lc} "))
                    || lt_lc.starts_with(&format!("pub {kind_lc} "))
                    || lt_lc.starts_with(&format!("export {kind_lc} "))
                    || lt_lc.starts_with(&format!("async {kind_lc} "))
                {
                    line_text.to_string()
                } else {
                    format!("{} {}", s.kind, line_text)
                };
                let snippet = if raw_snippet.len() <= 240 {
                    raw_snippet
                } else {
                    raw_snippet.chars().take(240).collect::<String>()
                };
                hits.push(Hit {
                    path: f.rel.clone(),
                    line: s.start_line as u32,
                    col: 1,
                    snippet,
                    is_definition: true,
                    score,
                });
            }
        }
        if hits.len() > before {
            stats.files_with_hits += 1;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_lines(
    ctx: &mut ToolContext<'_>,
    walk: &wxec_index::WalkResult,
    filter: &impl Fn(&str) -> bool,
    a: &SearchArgs,
    matcher: MatchMode<'_>,
    pat_lc: &str,
    pat_near: &str,
    hits: &mut Vec<Hit>,
    stats: &mut ScanStats,
) {
    for f in walk.files.iter() {
        if !filter(&f.rel) {
            continue;
        }
        if wxec_index::is_known_binary_ext(&f.rel) || f.size > 2_000_000 {
            stats.skipped_binary += 1;
            continue;
        }
        stats.scanned_files += 1;
        let Ok((text, binary)) = ctx.workspace.read_search_text(f) else {
            continue;
        };
        if binary {
            stats.skipped_binary += 1;
            continue;
        }
        let file_has_match = match &matcher {
            MatchMode::Regex(re) => re.is_match(&text),
            MatchMode::Literal(pat) => text.contains(pat),
        };
        if !file_has_match {
            continue;
        }
        if a.files_only {
            stats.files_with_hits += 1;
            hits.push(Hit {
                path: f.rel.clone(),
                line: 1,
                col: 1,
                snippet: String::new(),
                is_definition: false,
                score: 1,
            });
            continue;
        }

        let path_lc = f.rel.to_ascii_lowercase();
        let file_path_boost = path_boost_lc(&path_lc, pat_lc);
        let before = hits.len();
        for (idx, line) in text.lines().enumerate() {
            let (matched, col) = match &matcher {
                MatchMode::Regex(re) => match re.find(line) {
                    Some(m) => (true, m.start() + 1),
                    None => (false, 1),
                },
                MatchMode::Literal(pat) => match line.find(pat) {
                    Some(i) => (true, i + 1),
                    None => (false, 1),
                },
            };
            if matched {
                let trimmed = line.trim();
                let snippet = if trimmed.len() <= 240 {
                    trimmed.to_string()
                } else {
                    trimmed.chars().take(240).collect::<String>()
                };
                let is_definition = looks_like_definition(line);
                let mut score = if is_definition { 8 } else { 3 };
                if a.mode != "regex" && matches!(matcher, MatchMode::Literal(_)) {
                    let starts = line
                        .get(..pat_lc.len())
                        .is_some_and(|s| s.eq_ignore_ascii_case(pat_lc));
                    let ends = line
                        .get(line.len().saturating_sub(pat_lc.len())..)
                        .is_some_and(|s| s.eq_ignore_ascii_case(pat_lc));
                    if starts || ends {
                        score += 2;
                    } else {
                        let line_lc = line.to_ascii_lowercase();
                        if line_lc.contains(pat_near) {
                            score += 2;
                        }
                    }
                }
                score += file_path_boost;
                hits.push(Hit {
                    path: f.rel.clone(),
                    line: (idx + 1) as u32,
                    col: col as u32,
                    snippet,
                    is_definition,
                    score,
                });
            }
        }
        if hits.len() > before {
            stats.files_with_hits += 1;
        }
    }
}

const DEFN_PREFIXES: &[&str] = &[
    "fn ",
    "pub fn ",
    "async fn ",
    "pub async fn ",
    "def ",
    "class ",
    "struct ",
    "pub struct ",
    "enum ",
    "pub enum ",
    "trait ",
    "pub trait ",
    "impl ",
    "interface ",
    "pub interface ",
    "func ",
    "type ",
    "pub type ",
    "const ",
    "export ",
];

fn looks_like_definition(line: &str) -> bool {
    let t = line.trim_start();
    for &p in DEFN_PREFIXES {
        if t.starts_with(p) {
            return true;
        }
    }
    // Check embedded keyword without allocating Strings per keyword per line
    for &p in DEFN_PREFIXES {
        if let Some(pos) = t.find(p) {
            if pos > 0 && t.as_bytes()[pos - 1] == b' ' {
                return true;
            }
        }
    }
    false
}

fn path_boost_lc(rel_lc: &str, pat_lc: &str) -> i32 {
    let mut b = 0;
    if let Some(name) = rel_lc.rsplit('/').next() {
        if name.contains(pat_lc) {
            b += 2;
        }
    }
    if rel_lc.contains(pat_lc) {
        b += 1;
    }
    if rel_lc.starts_with("src/")
        || rel_lc.starts_with("crates/")
        || rel_lc.starts_with("lib/")
        || rel_lc.starts_with("pkg/")
        || rel_lc.starts_with("app/")
    {
        b += 1;
    }
    if wxec_index::is_test_or_vendor(rel_lc) {
        b -= 2;
    }
    b
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(SearchTool));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_looks_like_definition() {
        assert!(looks_like_definition("fn foo() {}"));
        assert!(looks_like_definition("pub fn bar() {}"));
        assert!(looks_like_definition("    pub async fn baz() {}"));
        assert!(looks_like_definition("struct MyStruct;"));
        assert!(looks_like_definition("export class TestClass"));
        assert!(looks_like_definition("something export const X = 1;"));
        assert!(!looks_like_definition("let x = fn_call();"));
        assert!(!looks_like_definition("println!(\"definitely not\");"));
    }

    #[test]
    fn test_format_line_numbers() {
        assert_eq!(format_line_numbers(&[1, 2, 3, 4, 5]), "1-5");
        assert_eq!(format_line_numbers(&[1, 3, 4, 5, 8]), "1,3-5,8");
        assert_eq!(format_line_numbers(&[42]), "42");
    }
}
