use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use wxec_core::error::{Result, WErr};
use wxec_core::handles::HandleStore;
use wxec_core::types::{Diagnostic, HandleRef, Item, ItemClass, Severity, TestResult, ToolResult};

pub fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        return format!("1 {noun}");
    }
    if noun.ends_with('s') || noun.ends_with("ch") || noun.ends_with("sh") || noun.ends_with('x') {
        format!("{n} {noun}es")
    } else {
        format!("{n} {noun}s")
    }
}

pub fn display_path(root: &Path, abs: &Path) -> String {
    if abs == root {
        ".".to_string()
    } else {
        wxec_index::Workspace::new(root).rel(abs)
    }
}

pub fn rel_path(root: &Path, p: &str) -> String {
    let path = Path::new(p);
    if path.is_absolute() {
        wxec_index::Workspace::new(root).rel(path)
    } else {
        p.replace('\\', "/")
    }
}

pub fn globset(glob: Option<&str>) -> Result<Option<GlobSet>> {
    let Some(pattern) = glob else {
        return Ok(None);
    };
    let glob = Glob::new(pattern).map_err(|e| {
        WErr::invalid_args(
            format!("bad glob '{pattern}': {e}"),
            "use a glob like 'src/**/*.rs'",
        )
    })?;
    let mut b = GlobSetBuilder::new();
    b.add(glob);
    b.build().map(Some).map_err(|e| {
        WErr::invalid_args(
            format!("bad glob '{pattern}': {e}"),
            "use a glob like 'src/**/*.rs'",
        )
    })
}

pub fn matches_glob(gs: Option<&GlobSet>, rel: &str) -> bool {
    match gs {
        Some(g) => g.is_match(rel),
        None => true,
    }
}

pub fn under(path: &str, base: &str) -> bool {
    base == "."
        || path == base
        || path
            .strip_prefix(base)
            .map(|r| r.starts_with('/'))
            .unwrap_or(false)
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| WErr::invalid_args("path has no parent directory", "use a file path"))?;
    if !parent.exists() {
        std::fs::create_dir_all(parent).map_err(|e| {
            WErr::new(
                "permission_denied",
                format!("create directory {}: {e}", parent.display()),
            )
        })?;
    }
    #[cfg(unix)]
    let existing_mode = std::fs::metadata(path).ok().map(|m| {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode()
    });
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = parent.join(format!(
        ".{}.wxec-{}-{}.tmp",
        path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string()),
        std::process::id(),
        nonce
    ));
    let write = || -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        #[cfg(unix)]
        if let Some(mode) = existing_mode {
            use std::os::unix::fs::PermissionsExt;
            let _ = f.set_permissions(std::fs::Permissions::from_mode(mode & 0o7777));
        }
        f.write_all(bytes)?;
        f.sync_all()?;
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(WErr::new(
            "permission_denied",
            format!("write {}: {e}", tmp.display()),
        ));
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        WErr::new(
            "permission_denied",
            format!("rename {}: {e}", path.display()),
        )
    })?;
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() || c == '~' {
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else if c == '\r' {
            // CRLF: normalize to LF, keep current line content.
            if matches!(chars.peek(), Some('\n')) {
                chars.next();
                out.push('\n');
                continue;
            }
            // Progress rewrites: keep only text after the last CR on this line.
            let mut tail = String::new();
            let mut saw_nl = false;
            while let Some(&nc) = chars.peek() {
                if nc == '\n' {
                    tail.push('\n');
                    chars.next();
                    saw_nl = true;
                    break;
                }
                if nc == '\r' {
                    if matches!(chars.clone().nth(1), Some('\n')) {
                        // treat upcoming CRLF as end of rewritten line
                        tail.push('\n');
                        chars.next(); // \r
                        chars.next(); // \n
                        saw_nl = true;
                        break;
                    }
                    tail.clear();
                    chars.next();
                    continue;
                }
                tail.push(nc);
                chars.next();
            }
            if let Some(pos) = out.rfind('\n') {
                out.truncate(pos + 1);
            } else {
                out.clear();
            }
            out.push_str(&tail);
            let _ = saw_nl;
        } else {
            out.push(c);
        }
    }
    out
}

pub fn normalize_diagnostics(root: &Path, diags: &mut Vec<Diagnostic>) {
    for d in diags.iter_mut() {
        d.file = rel_path(root, &d.file);
    }
    diags.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(a.file.cmp(&b.file))
            .then(a.line.cmp(&b.line))
            .then(a.col.cmp(&b.col))
            .then(a.code.cmp(&b.code))
            .then(a.message.cmp(&b.message))
    });
    diags.dedup_by(|a, b| {
        a.file == b.file
            && a.line == b.line
            && a.col == b.col
            && a.code == b.code
            && a.message == b.message
    });
}

pub fn diagnostics_into(
    result: &mut ToolResult,
    mut diags: Vec<Diagnostic>,
    root: &Path,
    handles: &mut HandleStore,
) -> Result<()> {
    normalize_diagnostics(root, &mut diags);
    if diags.is_empty() {
        return Ok(());
    }
    result.diagnostics.extend(diags.clone());

    let mut groups: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (i, d) in diags.iter().enumerate() {
        let key = (d.code.clone().unwrap_or_default(), d.message.clone());
        groups.entry(key).or_default().push(i);
    }

    let mut flooded: BTreeSet<usize> = BTreeSet::new();
    for idxs in groups.values() {
        if idxs.len() > 20 {
            let occurrences: Vec<String> = idxs.iter().map(|&i| diags[i].render()).collect();
            let href = handles.put("flood", occurrences.join("\n").as_bytes(), "text")?;
            let first = diags[idxs[0]].clone();
            let mut msg = format!("{} ({} occurrences)", first.message, idxs.len());
            if let Some(code) = &first.code {
                msg = format!("{code} {msg}");
            }
            result.push(Item::new(
                ItemClass::MustKeep,
                1000.0,
                vec![
                    format!("{} {}", first.severity.as_str().to_uppercase(), msg),
                    format!("...+{} lines ({})", idxs.len(), href.id),
                ],
            ));
            result.handle(HandleRef {
                id: href.id.clone(),
                kind: href.kind.clone(),
                bytes: href.bytes,
                hash: href.hash.clone(),
            });
            for &i in idxs {
                flooded.insert(i);
            }
        }
    }

    for (i, d) in diags.iter().enumerate() {
        if flooded.contains(&i) {
            continue;
        }
        let class = if d.severity == Severity::Error {
            ItemClass::MustKeep
        } else {
            ItemClass::Low
        };
        result.push(Item::new(
            class,
            if class == ItemClass::MustKeep {
                1000.0
            } else {
                5.0
            },
            vec![d.render()],
        ));
    }
    Ok(())
}

/// Keep all failure records (I1), plus a compact pass summary.
pub fn tests_into(result: &mut ToolResult, tests: &[TestResult], paths_relative: bool) {
    for t in tests.iter().filter(|t| t.status.is_failure()) {
        let mut line = format!("{} {}", t.status.as_str().to_uppercase(), t.id);
        if let Some(msg) = &t.message {
            let parts: Vec<String> = msg
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .take(3)
                .map(|l| l.to_string())
                .collect();
            let msg = parts.join("; ");
            if !msg.is_empty() {
                line.push_str(": ");
                line.push_str(&msg.chars().take(300).collect::<String>());
            }
        }
        if let Some(loc) = &t.location {
            let loc = if paths_relative {
                loc.replace('\\', "/")
            } else {
                loc.clone()
            };
            if !loc.is_empty() && !t.id.contains(&loc) {
                line.push_str(&format!(" ({loc})"));
            }
        }
        result.must(line);
    }
}

pub fn counts_of(tests: &[TestResult]) -> (usize, usize, usize) {
    let passed = tests
        .iter()
        .filter(|t| matches!(t.status, wxec_core::types::TestStatus::Pass))
        .count();
    let failed = tests.iter().filter(|t| t.status.is_failure()).count();
    let skipped = tests.len().saturating_sub(passed + failed);
    (passed, failed, skipped)
}

/// Keep only failure evidence: failing test blocks, failures list,
/// failed suite result lines, and terminal error. Passing suites,
/// progress dots, and noise are omitted.
pub fn compact_test_failures(
    result: &mut ToolResult,
    raw: &str,
    label: &str,
    handles: &mut HandleStore,
) -> Result<bool> {
    let lines = no_ansi_lines(raw);
    let mut selected: Vec<String> = Vec::new();
    let mut in_block = false;
    let mut in_failures = false;

    for line in lines {
        let t = line.trim();
        if t.starts_with("test result:") {
            if t.contains("FAILED") {
                selected.push(line.clone());
            }
            in_block = false;
            in_failures = false;
            continue;
        }
        if t.starts_with("---- ") && t.ends_with(" stdout ----") {
            in_block = true;
            in_failures = false;
            selected.push(line.clone());
            continue;
        }
        if t == "failures:" {
            in_block = false;
            in_failures = true;
            selected.push(line.clone());
            continue;
        }
        if in_block || in_failures {
            if t.is_empty() {
                continue;
            }
            if in_block
                && (t.starts_with("note: run with `RUST_BACKTRACE")
                    || t.starts_with("note: Some details are omitted"))
            {
                continue;
            }
            selected.push(line.clone());
            continue;
        }
        if t.starts_with("error: test failed")
            || t.starts_with("error: process didn't exit successfully")
        {
            selected.push(line);
        }
    }

    if selected.is_empty() {
        return Ok(false);
    }
    let h = handles.put(label, raw.as_bytes(), "text")?;
    result.handle(HandleRef {
        id: h.id.clone(),
        kind: h.kind.clone(),
        bytes: h.bytes,
        hash: h.hash.clone(),
    });
    result.push(Item::new(ItemClass::MustKeep, 1000.0, selected));
    Ok(true)
}

pub fn no_ansi_lines(text: &str) -> Vec<String> {
    strip_ansi(text)
        .lines()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn is_toolchain_frame(line: &str) -> bool {
    let l = line.trim();
    (l.starts_with("at ")
        || l.contains("::")
        || l.contains(".rs:")
        || l.contains(".go:")
        || l.contains("node_modules")
        || l.contains("site-packages"))
        && (l.contains("/rustc/")
            || l.contains("/rustlib/")
            || l.contains("library/std")
            || l.contains("library/core")
            || l.contains("library/alloc")
            || l.contains("site-packages/_pytest")
            || l.contains("node_modules/")
            || l.contains("internal/modules")
            || l.contains("<frozen "))
}

fn template_key(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '0' && matches!(chars.peek(), Some(&'x' | &'X')) {
            out.push(c);
            out.push(chars.next().unwrap());
            while matches!(chars.peek(), Some(&d) if d.is_ascii_hexdigit()) {
                chars.next();
            }
            out.push_str("<H>");
        } else if c.is_ascii_hexdigit()
            && c.is_ascii_alphabetic()
            && out.ends_with(|ch: char| ch.is_ascii_hexdigit() || ch == '-')
        {
            // keep hex-like uuid/hash segments normalized as part of hex runs below
            out.push(c);
        } else if c.is_ascii_digit() {
            while matches!(chars.peek(), Some(&d) if d.is_ascii_digit() || d == '.' || d == ':' || d == '-' || d == 'T' || d == 'Z' || d == '_')
            {
                chars.next();
            }
            out.push_str("<N>");
        } else if c.is_ascii_hexdigit() {
            // long hex/hash tokens
            let mut len = 1usize;
            let mut buf = String::from(c);
            while matches!(chars.peek(), Some(&d) if d.is_ascii_hexdigit()) {
                buf.push(chars.next().unwrap());
                len += 1;
            }
            if len >= 8 {
                out.push_str("<H>");
            } else {
                out.push_str(&buf);
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn strip_toolchain_frames(lines: Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut dropped = 0usize;
    for line in lines {
        if is_toolchain_frame(&line) {
            dropped += 1;
            continue;
        }
        if dropped > 0 {
            out.push(format!("...+{dropped} std frames"));
            dropped = 0;
        }
        out.push(line);
    }
    if dropped > 0 {
        out.push(format!("...+{dropped} std frames"));
    }
    out
}

pub fn is_warning_line(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    let t = l.trim_start();
    t.starts_with("warning")
        || t.contains("warning:")
        || t.contains("warning[")
        || t.contains(": warning:")
        || t.contains("] warning")
}

pub fn is_error_line(line: &str) -> bool {
    if is_warning_line(line) {
        return false;
    }
    let l = line.to_ascii_lowercase();
    let t = l.trim_start();
    t.starts_with("error")
        || t.contains("error[")
        || t.contains("error:")
        || t.contains(" failed")
        || t.starts_with("failed")
        || t.starts_with("fail ")
        || t.contains("failure")
        || t.contains("exception")
        || t.contains("panic")
        || t.contains("panicked at")
        || t.contains("assertion `")
        || t.contains("assertion failed")
        || t.contains("thread '") && t.contains("panicked")
        || t.contains("traceback")
        || t.contains("segfault")
        || t.contains("fatal")
        || t.contains("undefined reference")
        || t.contains("could not compile")
        || t.starts_with("--- fail")
        || t.starts_with("left:")
        || t.starts_with("right:")
        || line.contains("✗")
}

pub fn is_signal_summary_line(line: &str) -> bool {
    let t = line.trim().to_ascii_lowercase();
    t.contains("could not compile")
        || t.starts_with("error: aborting")
        || t.starts_with("test result:")
        || (t.starts_with("failures:") && t.len() <= "failures:".len() + 1)
        || t.contains("test failed")
}

fn is_error_context_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("-->")
        || t.starts_with('|')
        || t.starts_with("= help:")
        || t.starts_with("= note:")
        || t.starts_with("help:")
        || t.starts_with("note:")
        || (line.starts_with(' ') && t.contains('^'))
}

fn is_compact_signal_line(line: &str) -> bool {
    is_error_line(line) || is_signal_summary_line(line)
}

/// Generic compaction fallback (spec §7.5): head + error windows + tail.
pub fn compact_tail(
    result: &mut ToolResult,
    raw: &str,
    label: &str,
    handles: &mut HandleStore,
    force_full: bool,
) -> Result<()> {
    let mut lines = no_ansi_lines(raw);
    if lines.is_empty() {
        return Ok(());
    }
    lines = strip_toolchain_frames(lines);
    let raw_bytes = raw.len() as u64;
    let need_full = force_full || raw_bytes > 4096;
    let h = handles.put(label, raw.as_bytes(), "text")?;
    result.handle(HandleRef {
        id: h.id.clone(),
        kind: h.kind.clone(),
        bytes: h.bytes,
        hash: h.hash.clone(),
    });
    let href = h.id.clone();

    let head_n = 8.min(lines.len());
    let tail_n = 6.min(lines.len().saturating_sub(head_n));
    let mut selected: Vec<(usize, &str)> = Vec::new();
    // Prefer error-focused compaction: if errors/signals exist, keep only error windows and
    // terminal summaries; collapse warning bodies/noise (counts can remain elsewhere).
    let has_errors = lines.iter().any(|l| is_compact_signal_line(l));
    if !has_errors {
        for (i, l) in lines.iter().enumerate().take(head_n) {
            selected.push((i, l.as_str()));
        }
    }
    let mut skip_warning_until: usize = 0;
    for (i, l) in lines.iter().enumerate() {
        if !has_errors && i < head_n {
            continue;
        }
        if !has_errors && i >= lines.len().saturating_sub(tail_n) {
            selected.push((i, l.as_str()));
            continue;
        }
        if has_errors && i < skip_warning_until {
            continue;
        }
        if has_errors && is_warning_line(l) && !is_compact_signal_line(l) {
            // skip warning diagnostic block (message + context/caret/help tied to it)
            let mut k = i + 1;
            while k < lines.len() {
                let lk = lines[k].as_str();
                if is_compact_signal_line(lk) || is_warning_line(lk) {
                    break;
                }
                let t = lk.trim_start();
                let ctx = t.starts_with("-->")
                    || t.starts_with('|')
                    || t.starts_with('[')
                    || t.starts_with("= help:")
                    || t.starts_with("= note:")
                    || t.starts_with("help:")
                    || t.starts_with("note:")
                    || (lk.starts_with(' ')
                        && (t.contains('^')
                            || t.starts_with('~')
                            || !t.is_empty()
                                && t.chars().all(|c| {
                                    c == ' ' || c == '^' || c == '~' || c == '-' || c == '|'
                                })))
                    || lk.trim().is_empty();
                if !ctx {
                    break;
                }
                k += 1;
            }
            skip_warning_until = k;
            continue;
        }
        if is_compact_signal_line(l) {
            let start = i.saturating_sub(1);
            let end = (i + 8).min(lines.len());
            for (j, lj) in lines.iter().enumerate().take(end).skip(start) {
                let lj_s = lj.as_str();
                if has_errors
                    && (is_warning_line(lj_s) || (j >= skip_warning_until && j < i/*no*/))
                    && !is_compact_signal_line(lj_s)
                {
                    continue;
                }
                if has_errors && j > i {
                    // don't pull in warning blocks that start after signal? rare, but guard
                    if is_warning_line(lj_s) {
                        break;
                    }
                }
                if j < i && !(is_error_context_line(lj_s) || is_compact_signal_line(lj_s)) {
                    continue;
                }
                if j < i && has_errors && is_warning_line(lj_s) {
                    continue;
                }
                selected.push((j, lj_s));
                if j > i
                    && !is_compact_signal_line(lj_s)
                    && !is_error_context_line(lj_s)
                    && lj_s.trim().is_empty()
                {
                    break;
                }
            }
        }
    }
    if !has_errors {
        // tail already added above
    } else {
        // ensure terminal summaries near end are kept even if misclassified
        for (i, l) in lines.iter().enumerate().rev().take(12) {
            if is_signal_summary_line(l) {
                selected.push((i, l.as_str()));
            }
        }
    }
    if selected.is_empty() {
        for (i, l) in lines.iter().enumerate().take(head_n) {
            selected.push((i, l.as_str()));
        }
        for (i, l) in lines
            .iter()
            .enumerate()
            .skip(lines.len().saturating_sub(tail_n))
        {
            selected.push((i, l.as_str()));
        }
    }
    selected.sort_by_key(|(i, _)| *i);
    selected.dedup_by_key(|(i, _)| *i);

    let mut omitted = 0usize;
    let mut last_emitted: Option<usize> = None;
    let mut stream: Vec<(usize, &str, ItemClass)> = Vec::with_capacity(selected.len());
    for (i, line) in selected {
        let class = if is_compact_signal_line(line) || is_error_context_line(line) {
            ItemClass::MustKeep
        } else {
            ItemClass::Low
        };
        stream.push((i, line, class));
    }
    let mut j = 0usize;
    while j < stream.len() {
        let (i, line, class) = stream[j];
        if let Some(prev) = last_emitted {
            if i > prev + 1 {
                result.low(format!("...+{} lines ({href})", i - prev - 1));
                omitted += i - prev - 1;
            }
        }
        if class == ItemClass::Low {
            let mut k = j + 1;
            while k < stream.len()
                && stream[k].2 == ItemClass::Low
                && stream[k].1 == line
                && stream[k].0 == i + (k - j)
            {
                k += 1;
            }
            let mut count = k - j;
            if count == 1 {
                let tk = template_key(line);
                while k < stream.len()
                    && stream[k].2 == ItemClass::Low
                    && stream[k].0 == i + (k - j)
                    && template_key(stream[k].1) == tk
                {
                    k += 1;
                }
                count = k - j;
                if count > 2 {
                    result.low(format!("[template ×{count}] {tk}"));
                    last_emitted = Some(stream[k - 1].0);
                    j = k;
                    continue;
                }
                count = 1;
                k = j + 1;
            }
            if count > 1 {
                result.low(line.to_string());
                result.low(format!("  ×{count} repeated"));
            } else {
                result.push(Item::new(ItemClass::Low, 2.0, vec![line.to_string()]));
            }
            last_emitted = Some(stream[k - 1].0);
            j = k;
            continue;
        }
        result.push(Item::new(
            ItemClass::MustKeep,
            1000.0,
            vec![line.to_string()],
        ));
        last_emitted = Some(i);
        j += 1;
    }
    if let Some(last) = last_emitted {
        if last + 1 < lines.len() {
            omitted += lines.len() - last - 1;
        }
    }
    let full_line = format!("full: {href}");
    if force_full || need_full {
        if !result
            .items
            .iter()
            .any(|i| i.lines.iter().any(|l| l.starts_with("full: ~")))
        {
            result.must(full_line);
        }
    } else if omitted > 0 {
        result.must(format!("...+{omitted} lines ({href})"));
    }
    Ok(())
}

pub fn path_buf(root: &Path, rel: &str) -> PathBuf {
    if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        root.join(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use wxec_core::handles::HandleStore;
    use wxec_core::redact::Redactor;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wxec-util-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn compact_tail_keeps_error_context_and_dedupes() {
        let dir = tmp("compact");
        let mut hs = HandleStore::open("s", &dir, 3600, 8 << 20, Redactor::new()).unwrap();
        let mut tr = ToolResult::new("run", "exit 1".to_string());
        let raw = (0..30).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n")
            + "\nerror[E0425]: cannot find value `x`\n --> src/lib.rs:2:5\n  |\n2 |     x\n  |     ^ not found\n"
            + &"repeat\n".repeat(5)
            + "at std::panicking::begin_panic (/rustc/hash/library/std/src/panicking.rs:1:1)\n"
            + "tail\n";
        compact_tail(&mut tr, &raw, "run", &mut hs, true).unwrap();
        let text = tr
            .items
            .iter()
            .flat_map(|i| i.lines.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("error[E0425]"), "{text}");
        assert!(text.contains("--> src/lib.rs:2:5"), "{text}");
        assert!(text.contains("^ not found"), "{text}");
        assert!(text.contains("repeat"), "{text}");
        assert!(
            text.contains("×5 repeated") || text.matches("repeat").count() <= 3,
            "{text}"
        );
        assert!(
            text.contains("...+1 std frames") || !text.contains("rustc/hash"),
            "{text}"
        );
        assert_eq!(text.matches("full: ~").count(), 1, "{text}");
    }

    #[test]
    fn compact_tail_collapses_template_runs() {
        let dir = tmp("tpl");
        let mut hs = HandleStore::open("s", &dir, 3600, 8 << 20, Redactor::new()).unwrap();
        let mut tr = ToolResult::new("run", "exit 0".to_string());
        let raw =
            "start\nretry 1 in 10ms\nretry 2 in 20ms\nretry 3 in 30ms\nretry 4 in 40ms\nend\n";
        compact_tail(&mut tr, raw, "run", &mut hs, false).unwrap();
        let text = tr
            .items
            .iter()
            .flat_map(|i| i.lines.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("[template ×4]"), "{text}");
        assert!(!text.contains("retry 3 in 30ms"), "{text}");
    }

    #[test]
    fn compact_tail_never_drops_error_lines() {
        let dir = tmp("nodrop");
        let mut hs = HandleStore::open("s", &dir, 3600, 8 << 20, Redactor::new()).unwrap();
        let mut tr = ToolResult::new("run", "exit 1".to_string());
        let mut raw = String::new();
        for i in 0..200 {
            raw.push_str(&format!("noise {i}\n"));
        }
        raw.push_str("error: first failure\n  --> a.rs:1:1\n");
        for i in 0..200 {
            raw.push_str(&format!("mid {i}\n"));
        }
        raw.push_str("FATAL: second\n");
        for i in 0..200 {
            raw.push_str(&format!("tail {i}\n"));
        }
        compact_tail(&mut tr, &raw, "run", &mut hs, true).unwrap();
        let text = tr
            .items
            .iter()
            .flat_map(|i| i.lines.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("error: first failure"), "{text}");
        assert!(text.contains("--> a.rs:1:1"), "{text}");
        assert!(text.contains("FATAL: second"), "{text}");
    }

    #[test]
    fn compact_tail_template_preserves_interleaved_errors() {
        let dir = tmp("tplerr");
        let mut hs = HandleStore::open("s", &dir, 3600, 8 << 20, Redactor::new()).unwrap();
        let mut tr = ToolResult::new("run", "exit 1".to_string());
        let raw = "retry 1\nretry 2\nretry 3\nerror: boom\nretry 4\nretry 5\nretry 6\n";
        compact_tail(&mut tr, raw, "run", &mut hs, true).unwrap();
        let text = tr
            .items
            .iter()
            .flat_map(|i| i.lines.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("error: boom"), "{text}");
    }

    #[test]
    fn compact_tail_never_drops_many_scattered_errors() {
        let dir = tmp("nodrop2");
        let mut hs = HandleStore::open("s", &dir, 3600, 8 << 20, Redactor::new()).unwrap();
        let mut tr = ToolResult::new("run", "exit 1".to_string());
        let mut raw = String::new();
        for i in 0..120 {
            if i % 17 == 0 {
                raw.push_str(&format!("error: e{i}\n"));
            } else {
                raw.push_str(&format!("line {i}\n"));
            }
        }
        compact_tail(&mut tr, &raw, "run", &mut hs, true).unwrap();
        let text = tr
            .items
            .iter()
            .flat_map(|i| i.lines.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        for i in 0..120 {
            if i % 17 == 0 {
                assert!(
                    text.contains(&format!("error: e{i}")),
                    "missing e{i} in {text}"
                );
            }
        }
    }

    #[test]
    fn compact_tail_collapses_warning_bodies_when_error_present() {
        let dir = tmp("warnerr");
        let mut hs = HandleStore::open("s", &dir, 3600, 8 << 20, Redactor::new()).unwrap();
        let mut tr = ToolResult::new("run", "exit 1".to_string());
        let mut raw = String::new();
        for i in 0..30 {
            raw.push_str(&format!(
                "warning: unused import {i}\n --> src/mod{i}.rs:{i}:5\n  |\n  | use foo;\n\n"
            ));
        }
        raw.push_str("error[E0308]: mismatched types\n --> src/main.rs:42:18\n  |\n42 |     let x: i32 = \"no\";\n  |            ---   ^^^^ expected `i32`, found `&str`\n\n");
        raw.push_str("error: could not compile `app` due to 1 previous error\n");
        compact_tail(&mut tr, &raw, "run", &mut hs, true).unwrap();
        let text = tr
            .items
            .iter()
            .flat_map(|i| i.lines.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("error[E0308]"), "{text}");
        assert!(text.contains("src/main.rs:42:18"), "{text}");
        assert!(text.contains("could not compile"), "{text}");
        assert!(!text.contains("warning: unused import 10"), "{text}");
        assert!(!text.contains("src/mod10.rs:10:5"), "{text}");
    }
}
