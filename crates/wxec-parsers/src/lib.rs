use std::path::Path;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use wxec_core::parsers::{ParsedRun, ParserSet, RunCounts};
use wxec_core::types::{Diagnostic, Exit, Severity, TestResult, TestStatus};

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

pub struct ParserEntry {
    pub name: String,
    pub priority: i32,
    pub kind: Kind,
    pub inject: Vec<String>,
    pub inject_if_absent: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Cargo,
    Pytest,
    Jest,
    Vitest,
    Tsc,
    Eslint,
    Ruff,
    GoTest,
    GoVet,
    Zig,
    Maven,
    Rspec,
    Rubocop,
    Toml(Box<TomlParser>),
}

pub struct ParserRegistry {
    entries: Vec<ParserEntry>,
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ParserRegistry {
    pub fn new() -> Self {
        Self::load(None, false)
    }

    pub fn for_root(root: &Path, no_project_config: bool) -> Self {
        let include_project = !no_project_config && wxec_core::config::is_trusted(root);
        Self::load(Some(root), include_project)
    }

    fn load(root: Option<&Path>, include_project: bool) -> Self {
        let native = [
            (
                "cargo",
                90,
                Kind::Cargo,
                vec!["--message-format=json"],
                Some("--message-format"),
            ),
            ("pytest", 90, Kind::Pytest, vec![], None),
            ("jest", 90, Kind::Jest, vec!["--json"], Some("--json")),
            (
                "vitest",
                90,
                Kind::Vitest,
                vec!["--reporter=json"],
                Some("--reporter"),
            ),
            ("tsc", 90, Kind::Tsc, vec![], None),
            (
                "eslint",
                90,
                Kind::Eslint,
                vec!["-f", "json"],
                Some("--format"),
            ),
            (
                "ruff",
                90,
                Kind::Ruff,
                vec!["--output-format=json"],
                Some("--output-format"),
            ),
            ("go-test", 90, Kind::GoTest, vec![], None),
            ("go-vet", 90, Kind::GoVet, vec![], None),
            ("zig", 90, Kind::Zig, vec![], None),
            ("maven", 90, Kind::Maven, vec![], None),
            ("rspec", 90, Kind::Rspec, vec![], None),
            ("rubocop", 90, Kind::Rubocop, vec![], None),
        ];
        let mut entries: Vec<ParserEntry> = native
            .into_iter()
            .map(|(name, priority, kind, inject, guard)| ParserEntry {
                name: name.to_string(),
                priority,
                kind,
                inject: inject.into_iter().map(str::to_string).collect(),
                inject_if_absent: guard,
            })
            .collect();

        for dir in Self::rule_dirs(root, include_project) {
            if let Ok(read) = std::fs::read_dir(&dir) {
                let mut paths: Vec<_> = read.flatten().map(|e| e.path()).collect();
                paths.sort();
                for path in paths {
                    if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                        continue;
                    }
                    match std::fs::symlink_metadata(&path) {
                        Ok(m) if m.file_type().is_symlink() => {
                            eprintln!("# parser rule {} ignored: symlink", path.display());
                            continue;
                        }
                        Ok(m) if !m.file_type().is_file() => continue,
                        Err(e) => {
                            eprintln!("# parser rule {} ignored: {e}", path.display());
                            continue;
                        }
                        _ => {}
                    }
                    match TomlParser::load(&path) {
                        Ok(p) => entries.push(ParserEntry {
                            name: p.name.clone(),
                            priority: p.priority,
                            inject: p.inject_args.clone(),
                            inject_if_absent: None,
                            kind: Kind::Toml(Box::new(p)),
                        }),
                        Err(e) => eprintln!("# parser rule {} ignored: {e}", path.display()),
                    }
                }
            }
        }
        entries.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.name.cmp(&b.name)));
        Self { entries }
    }

    pub fn with_toml(mut self, parser: TomlParser) -> Self {
        self.entries.push(ParserEntry {
            name: parser.name.clone(),
            priority: parser.priority,
            inject: parser.inject_args.clone(),
            inject_if_absent: None,
            kind: Kind::Toml(Box::new(parser)),
        });
        self.entries
            .sort_by(|a, b| b.priority.cmp(&a.priority).then(a.name.cmp(&b.name)));
        self
    }

    pub fn names(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.name.clone()).collect()
    }

    fn rule_dirs(root: Option<&Path>, include_project: bool) -> Vec<std::path::PathBuf> {
        let mut dirs = Vec::new();
        let config_dir = wxec_core::paths::config_dir();
        dirs.push(config_dir.join("rules.d"));
        dirs.push(config_dir.join("parsers.d"));
        if include_project {
            if let Some(root) = root {
                dirs.push(root.join(".wxec").join("rules.d"));
                dirs.push(root.join(".wxec").join("parsers.d"));
            }
        }
        dirs
    }

    fn matching(&self, argv: &[String]) -> Vec<&ParserEntry> {
        self.entries
            .iter()
            .filter(|e| matches_entry(e, argv))
            .collect()
    }
}

impl ParserSet for ParserRegistry {
    fn inject_args(&self, argv: &[String]) -> Vec<String> {
        let Some(entry) = self.matching(argv).into_iter().next() else {
            return argv.to_vec();
        };
        if entry.inject.is_empty() {
            return argv.to_vec();
        }
        if let Some(guard) = entry.inject_if_absent {
            if argv
                .iter()
                .any(|a| a == guard || a.starts_with(&format!("{guard}=")))
            {
                return argv.to_vec();
            }
        }
        // For ruff / eslint only inject in the expected subcommand context.
        if matches!(entry.kind, Kind::Ruff) && !argv.iter().any(|a| a == "check") {
            return argv.to_vec();
        }
        let mut flags: Vec<String> = entry
            .inject
            .iter()
            .filter(|f| !argv.iter().any(|a| a == *f))
            .cloned()
            .collect();
        if flags.is_empty() {
            return argv.to_vec();
        }
        // Insert before `--` so tool-specific args are not passed to child/args after separator (WS1.1).
        let pos = argv.iter().position(|a| a == "--").unwrap_or(argv.len());
        let mut out = Vec::with_capacity(argv.len() + flags.len());
        out.extend_from_slice(&argv[..pos]);
        out.append(&mut flags);
        out.extend_from_slice(&argv[pos..]);
        out
    }

    fn parse(&self, argv: &[String], exit: &Exit, stdout: &str, stderr: &str) -> Option<ParsedRun> {
        for entry in self.matching(argv) {
            let parsed = match &entry.kind {
                Kind::Cargo => parse_cargo(argv, exit, stdout),
                Kind::Pytest => parse_pytest(stdout, stderr),
                Kind::Jest => parse_jest(stdout),
                Kind::Vitest => parse_vitest(stdout),
                Kind::Tsc => parse_tsc(exit, stdout, stderr),
                Kind::Eslint => parse_eslint(exit, stdout),
                Kind::Ruff => parse_ruff(exit, stdout),
                Kind::GoTest => parse_go_test(stdout),
                Kind::GoVet => parse_go_vet(exit, stdout, stderr),
                Kind::Zig => parse_zig(exit, stdout, stderr),
                Kind::Maven => parse_maven(exit, stdout, stderr),
                Kind::Rspec => parse_rspec(exit, stdout, stderr),
                Kind::Rubocop => parse_rubocop(exit, stdout, stderr),
                Kind::Toml(p) => p.parse(stdout, stderr),
            };
            if let Some(mut run) = parsed {
                run.parser = Some(entry.name.clone());
                return Some(run);
            }
        }
        None
    }
}

fn base(argv0: &str) -> String {
    let name = Path::new(argv0)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| argv0.to_string());
    name.strip_suffix(".exe").unwrap_or(&name).to_string()
}

fn cargo_sub(argv: &[String]) -> Option<String> {
    let mut iter = argv.iter().skip(1).peekable();
    while let Some(a) = iter.next() {
        if a.starts_with('+') {
            iter.next();
            continue;
        }
        if a.starts_with('-') {
            continue;
        }
        return Some(a.clone());
    }
    None
}

fn matches_entry(entry: &ParserEntry, argv: &[String]) -> bool {
    if argv.is_empty() {
        return false;
    }
    let b = base(&argv[0]);
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    match &entry.kind {
        Kind::Cargo => {
            b == "cargo"
                && matches!(
                    cargo_sub(argv).as_deref(),
                    Some("build" | "check" | "clippy" | "test")
                )
        }
        Kind::Pytest => {
            b == "pytest"
                || b == "py.test"
                || (b.starts_with("python") && args.windows(2).any(|w| w == ["-m", "pytest"]))
        }
        Kind::Jest => b == "jest",
        Kind::Vitest => b == "vitest",
        Kind::Tsc => b == "tsc",
        Kind::Eslint => b == "eslint",
        Kind::Ruff => b == "ruff" && args.contains(&"check"),
        Kind::GoTest => b == "go" && args.get(1) == Some(&"test"),
        Kind::GoVet => b == "go" && matches!(args.get(1), Some(&"vet") | Some(&"build")),
        Kind::Zig => b == "zig" && matches!(args.get(1), Some(&"build") | Some(&"test")),
        Kind::Maven => b == "mvn" || b == "mvnw",
        Kind::Rspec => {
            b == "rspec" || (b == "bundle" && args.windows(2).any(|w| w == ["exec", "rspec"]))
        }
        Kind::Rubocop => {
            b == "rubocop" || (b == "bundle" && args.windows(2).any(|w| w == ["exec", "rubocop"]))
        }
        Kind::Toml(p) => p.matches(argv),
    }
}

// ---------------------------------------------------------------------------
// Native parsers
// ---------------------------------------------------------------------------

fn parse_cargo(argv: &[String], exit: &Exit, stdout: &str) -> Option<ParsedRun> {
    let mut run = ParsedRun::default();
    let mut saw_json = false;
    let mut passed = 0;
    let mut failed = 0;
    let mut ignored = 0;

    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        saw_json = true;
        match v.get("reason").and_then(Value::as_str) {
            Some("compiler-message") => {
                if let Some(msg) = v.get("message") {
                    if let Some(d) = cargo_diagnostic(msg) {
                        run.diagnostics.push(d);
                    }
                }
            }
            Some("compiler-artifact") | Some("build-finished") => {}
            _ => {
                if v.get("type").and_then(Value::as_str) == Some("test") {
                    let event = v.get("event").and_then(Value::as_str).unwrap_or("");
                    let name = v
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("test")
                        .to_string();
                    let stdout_msg = v
                        .get("stdout")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                    let status = match event {
                        "ok" => {
                            passed += 1;
                            TestStatus::Pass
                        }
                        "failed" | "timeout" => {
                            failed += 1;
                            if event == "timeout" {
                                TestStatus::Timeout
                            } else {
                                TestStatus::Fail
                            }
                        }
                        "ignored" => {
                            ignored += 1;
                            TestStatus::Skip
                        }
                        _ => continue,
                    };
                    run.tests.push(TestResult {
                        id: name,
                        status,
                        duration_ms: None,
                        message: stdout_msg,
                        location: None,
                    });
                }
            }
        }
    }

    if !saw_json {
        return None;
    }

    // `cargo test --message-format=json` still emits libtest's human output.
    if run.tests.is_empty() {
        let re_test = Regex::new(r"^test (.+?) \.\.\. (ok|FAILED|ignored)").ok();
        let re_result =
            Regex::new(r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored")
                .ok();
        let mut saw_result = false;
        let mut summed = RunCounts::default();
        let mut panic_msg: Option<(String, Vec<String>)> = None;
        let mut messages: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for line in stdout.lines() {
            let t = line.trim();
            if let Some(caps) = re_result.as_ref().and_then(|re| re.captures(t)) {
                saw_result = true;
                summed.passed += caps
                    .get(1)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(0);
                summed.failed += caps
                    .get(2)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(0);
                summed.ignored += caps
                    .get(3)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(0);
                continue;
            }
            if let Some(rest) = t.strip_prefix("---- ") {
                if let Some(name) = rest.strip_suffix(" stdout ----") {
                    panic_msg = Some((name.to_string(), Vec::new()));
                    continue;
                }
            }
            if let Some((name, msg)) = panic_msg.as_mut() {
                if t == "failures:" || t.starts_with("failures:") {
                    let joined = msg.join("; ");
                    if !joined.is_empty() {
                        messages.insert(name.clone(), joined.chars().take(400).collect());
                    }
                    panic_msg = None;
                    continue;
                }
                if t.starts_with("note: run with")
                    || t.starts_with("thread '")
                    || t.starts_with("stack backtrace")
                    || t.starts_with("---- ")
                {
                    continue;
                }
                if !t.is_empty() && msg.len() < 3 {
                    msg.push(t.chars().take(240).collect());
                    continue;
                }
            }
            if let Some(caps) = re_test.as_ref().and_then(|re| re.captures(t)) {
                let name = caps
                    .get(1)
                    .map(|m| m.as_str())
                    .unwrap_or("test")
                    .to_string();
                let status = match caps.get(2).map(|m| m.as_str()).unwrap_or("") {
                    "ok" => TestStatus::Pass,
                    "ignored" => TestStatus::Skip,
                    _ => TestStatus::Fail,
                };
                run.tests.push(TestResult {
                    id: name,
                    status,
                    duration_ms: None,
                    message: None,
                    location: None,
                });
            }
        }
        if saw_result {
            passed = summed.passed;
            failed = summed.failed;
            ignored = summed.ignored;
        } else {
            let (p, f, i) = run.counts();
            passed = p;
            failed = f;
            ignored = i;
        }
        if let Some((name, msg)) = panic_msg {
            let joined = msg.join("; ");
            if !joined.is_empty() {
                messages.insert(name, joined.chars().take(400).collect());
            }
        }
        for t in run.tests.iter_mut() {
            if let Some(msg) = messages.remove(&t.id) {
                t.message = Some(msg);
            }
        }
    }

    if !run.tests.is_empty() || passed > 0 || failed > 0 || ignored > 0 {
        run.totals = Some(RunCounts {
            passed,
            failed,
            ignored,
        });
    }

    let errors = run
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warnings = run
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .count();
    let mut parts = Vec::new();
    if !run.tests.is_empty() || failed > 0 || passed > 0 || ignored > 0 {
        if passed > 0 {
            parts.push(format!("{passed} passed"));
        }
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        if ignored > 0 {
            parts.push(format!("{ignored} ignored"));
        }
    } else if exit.is_success() {
        let sub = cargo_sub(argv).unwrap_or_else(|| "cargo".to_string());
        parts.push(format!("{sub} ok"));
    } else if errors == 0 {
        parts.push("failed".to_string());
    }
    if errors > 0 {
        parts.push(count_label(errors, "error"));
    }
    if warnings > 0 {
        parts.push(count_label(warnings, "warning"));
    }
    if !parts.is_empty() {
        run.summary = Some(parts.join(", "));
    }
    Some(run)
}

fn count_label(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else if noun.ends_with("ch") || noun.ends_with('s') || noun.ends_with('x') {
        format!("{n} {noun}es")
    } else {
        format!("{n} {noun}s")
    }
}

fn with_hints(message: String, hints: Vec<String>) -> String {
    let mut out = message;
    for h in hints.into_iter().take(2) {
        let add = format!("; help: {h}");
        if out.chars().count() + add.chars().count() > 300 {
            break;
        }
        out.push_str(&add);
    }
    out
}

fn child_hints(msg: &Value) -> Vec<String> {
    msg.get("children")
        .and_then(Value::as_array)
        .map(|children| {
            children
                .iter()
                .filter(|c| {
                    matches!(
                        c.get("level").and_then(Value::as_str),
                        Some("help") | Some("note")
                    )
                })
                .filter_map(|c| c.get("message").and_then(Value::as_str))
                .filter(|m| !m.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn cargo_diagnostic(msg: &Value) -> Option<Diagnostic> {
    let level = msg.get("level").and_then(Value::as_str).unwrap_or("error");
    let severity = match level {
        "error" | "error: internal compiler error" => Severity::Error,
        "warning" => Severity::Warn,
        _ => return None,
    };
    let message = with_hints(
        msg.get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        child_hints(msg),
    );
    let code = msg
        .get("code")
        .and_then(|c| c.get("code"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let spans = msg.get("spans").and_then(Value::as_array);
    let mut file = String::new();
    let mut line = 1u32;
    let mut col = None;
    if let Some(spans) = spans {
        let primary = spans
            .iter()
            .find(|s| {
                s.get("is_primary")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .or_else(|| spans.first());
        if let Some(span) = primary {
            file = span
                .get("file_name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            line = span.get("line_start").and_then(Value::as_u64).unwrap_or(1) as u32;
            col = span
                .get("column_start")
                .and_then(Value::as_u64)
                .map(|c| c as u32);
        }
    }
    if file.is_empty() {
        return None;
    }
    Some(Diagnostic {
        file,
        line,
        col,
        severity,
        code,
        message,
        tool: Some("cargo".to_string()),
    })
}

fn parse_tsc(exit: &Exit, stdout: &str, stderr: &str) -> Option<ParsedRun> {
    let re_paren =
        Regex::new(r"^(.*)\((\d+),(\d+)\):\s+(error|warning)\s+([A-Za-z]+\d+)?:?\s*(.*)$").ok()?;
    let re_colon =
        Regex::new(r"^(.*):(\d+):(\d+)\s+-\s+(error|warning)\s+([A-Za-z]+\d+)?:?\s*(.*)$").ok()?;
    let mut run = ParsedRun::default();
    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim();
        let caps = re_paren.captures(line).or_else(|| re_colon.captures(line));
        let Some(caps) = caps else { continue };
        let severity = match caps.get(4).map(|m| m.as_str()).unwrap_or("error") {
            "warning" => Severity::Warn,
            _ => Severity::Error,
        };
        run.diagnostics.push(Diagnostic {
            file: caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
            line: caps
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(1),
            col: caps.get(3).and_then(|m| m.as_str().parse().ok()),
            severity,
            code: caps.get(5).map(|m| m.as_str().to_string()),
            message: caps.get(6).map(|m| m.as_str()).unwrap_or("").to_string(),
            tool: Some("tsc".to_string()),
        });
    }
    if run.diagnostics.is_empty() {
        if exit.is_success() {
            run.summary = Some("tsc ok".to_string());
            return Some(run);
        }
        return None;
    }
    summarize_diagnostics(&mut run);
    Some(run)
}

fn parse_eslint(exit: &Exit, stdout: &str) -> Option<ParsedRun> {
    let root: Value = serde_json::from_str(stdout.trim()).ok()?;
    let files = root.as_array()?;
    let mut run = ParsedRun::default();
    for file in files {
        let path = file
            .get("filePath")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let Some(messages) = file.get("messages").and_then(Value::as_array) else {
            continue;
        };
        for m in messages {
            let severity = match m.get("severity").and_then(Value::as_u64).unwrap_or(2) {
                1 => Severity::Warn,
                _ => Severity::Error,
            };
            run.diagnostics.push(Diagnostic {
                file: path.clone(),
                line: m.get("line").and_then(Value::as_u64).unwrap_or(1) as u32,
                col: m.get("column").and_then(Value::as_u64).map(|c| c as u32),
                severity,
                code: m.get("ruleId").and_then(Value::as_str).map(str::to_string),
                message: with_hints(
                    m.get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    m.get("suggestions")
                        .and_then(Value::as_array)
                        .map(|s| {
                            s.iter()
                                .filter_map(|x| x.get("desc").and_then(Value::as_str))
                                .filter(|d| !d.is_empty())
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
                tool: Some("eslint".to_string()),
            });
        }
    }
    if run.diagnostics.is_empty() {
        if exit.is_success() {
            run.summary = Some("eslint ok".to_string());
            return Some(run);
        }
        return None;
    }
    summarize_diagnostics(&mut run);
    Some(run)
}

fn parse_ruff(exit: &Exit, stdout: &str) -> Option<ParsedRun> {
    let root: Value = serde_json::from_str(stdout.trim()).ok()?;
    let items = root.as_array()?;
    let mut run = ParsedRun::default();
    for item in items {
        let file = item
            .get("filename")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let loc = item.get("location");
        let line = loc
            .and_then(|l| l.get("row"))
            .and_then(Value::as_u64)
            .unwrap_or(1) as u32;
        let col = loc
            .and_then(|l| l.get("column"))
            .and_then(Value::as_u64)
            .map(|c| c as u32);
        run.diagnostics.push(Diagnostic {
            file,
            line,
            col,
            severity: Severity::Warn,
            code: item.get("code").and_then(Value::as_str).map(str::to_string),
            message: item
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            tool: Some("ruff".to_string()),
        });
    }
    if run.diagnostics.is_empty() {
        if exit.is_success() {
            run.summary = Some("ruff ok".to_string());
            return Some(run);
        }
        return None;
    }
    summarize_diagnostics(&mut run);
    Some(run)
}

fn parse_pytest(stdout: &str, stderr: &str) -> Option<ParsedRun> {
    let re_summary = Regex::new(r"=+\s*(.*?)\s+in\s+[0-9.]+s\s*=+").ok()?;
    let mut run = ParsedRun::default();
    let mut summary_line = None;
    let mut saw_fail = false;
    for line in stdout.lines().chain(stderr.lines()) {
        let t = line.trim();
        if let Some(caps) = re_summary.captures(t) {
            summary_line = caps.get(1).map(|m| m.as_str().to_string());
            continue;
        }
        for prefix in ["FAILED ", "ERROR "] {
            if let Some(rest) = t.strip_prefix(prefix) {
                saw_fail = true;
                let (id, message) = match rest.split_once(" - ") {
                    Some((a, b)) => (a.trim().to_string(), Some(b.trim().to_string())),
                    None => (rest.trim().to_string(), None),
                };
                let status = if prefix == "ERROR " {
                    TestStatus::Error
                } else {
                    TestStatus::Fail
                };
                run.tests.push(TestResult {
                    id,
                    status,
                    duration_ms: None,
                    message,
                    location: None,
                });
            }
        }
    }
    if !saw_fail && summary_line.is_none() {
        return None;
    }
    if let Some(ref s) = summary_line {
        if !saw_fail && s.contains("no tests ran") {
            run.summary = Some("pytest: no tests ran".to_string());
            return Some(run);
        }
    }
    run.summary = summary_line;
    Some(run)
}

fn parse_jest(stdout: &str) -> Option<ParsedRun> {
    let start = stdout.find('{')?;
    let end = stdout.rfind('}')?;
    if end <= start {
        return None;
    }
    let root: Value = serde_json::from_str(&stdout[start..=end]).ok()?;
    parse_jest_like(&root)
}

fn parse_vitest(stdout: &str) -> Option<ParsedRun> {
    let start = stdout.find('{')?;
    let end = stdout.rfind('}')?;
    if end <= start {
        return None;
    }
    let root: Value = serde_json::from_str(&stdout[start..=end]).ok()?;
    parse_jest_like(&root)
}

fn parse_jest_like(root: &Value) -> Option<ParsedRun> {
    // Accept JSON reporter output even when empty so success summaries/counts are preserved.
    if root.get("testResults").is_none() && root.get("numTotalTests").is_none() {
        return None;
    }
    let results = root
        .get("testResults")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut run = ParsedRun::default();
    if results.is_empty() {
        run.summary = Some("0 tests".to_string());
        return Some(run);
    }
    for file in &results {
        let file_name = file
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let assertions = file
            .get("assertionResults")
            .or_else(|| file.get("tests"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for a in assertions {
            let title = a
                .get("fullName")
                .or_else(|| a.get("name"))
                .or_else(|| a.get("title"))
                .and_then(Value::as_str)
                .unwrap_or("test")
                .to_string();
            let status = match a.get("status").and_then(Value::as_str).unwrap_or("") {
                "passed" => TestStatus::Pass,
                "failed" => TestStatus::Fail,
                "pending" | "skipped" | "todo" | "disabled" => TestStatus::Skip,
                "error" => TestStatus::Error,
                _ => TestStatus::Skip,
            };
            let message = a
                .get("failureMessages")
                .and_then(Value::as_array)
                .and_then(|m| m.first())
                .and_then(Value::as_str)
                .map(|s| s.lines().next().unwrap_or("").to_string());
            let duration_ms = a.get("duration").and_then(Value::as_u64);
            run.tests.push(TestResult {
                id: title,
                status,
                duration_ms,
                message,
                location: if file_name.is_empty() {
                    None
                } else {
                    Some(file_name.clone())
                },
            });
        }
    }
    if run.tests.is_empty() {
        run.summary = Some("0 tests".to_string());
        return Some(run);
    }
    let (passed, failed, skipped) = run.counts();
    let mut parts = Vec::new();
    if passed > 0 {
        parts.push(format!("{passed} passed"));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} skipped"));
    }
    run.summary = Some(parts.join(", "));
    Some(run)
}

fn parse_go_test(stdout: &str) -> Option<ParsedRun> {
    let re_test = Regex::new(r"^\s*--- (FAIL|PASS|SKIP): (\S+)").ok()?;
    let re_diag = Regex::new(r"^\s*(?:\./)?([^:\s]+\.go):(\d+):(\d+):\s*(.*)$").ok()?;
    let re_pkg = Regex::new(r"^(ok|FAIL)\s+(\S+)").ok();
    let mut run = ParsedRun::default();
    let mut saw = false;
    let mut pkg_ok = 0usize;
    let mut pkg_fail = 0usize;
    for line in stdout.lines() {
        if let Some(caps) = re_test.captures(line) {
            saw = true;
            let status = match caps.get(1).map(|m| m.as_str()).unwrap_or("") {
                "FAIL" => TestStatus::Fail,
                "PASS" => TestStatus::Pass,
                _ => TestStatus::Skip,
            };
            run.tests.push(TestResult {
                id: caps
                    .get(2)
                    .map(|m| m.as_str())
                    .unwrap_or("test")
                    .to_string(),
                status,
                duration_ms: None,
                message: None,
                location: None,
            });
            continue;
        }
        if let Some(caps) = re_diag.captures(line) {
            saw = true;
            run.diagnostics.push(Diagnostic {
                file: caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
                line: caps
                    .get(2)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(1),
                col: caps.get(3).and_then(|m| m.as_str().parse().ok()),
                severity: Severity::Error,
                code: None,
                message: caps.get(4).map(|m| m.as_str()).unwrap_or("").to_string(),
                tool: Some("go".to_string()),
            });
            continue;
        }
        if let Some(caps) = re_pkg.as_ref().and_then(|re| re.captures(line.trim())) {
            saw = true;
            match caps.get(1).map(|m| m.as_str()).unwrap_or("") {
                "ok" => pkg_ok += 1,
                "FAIL" => pkg_fail += 1,
                _ => {}
            }
        }
    }
    if !saw {
        return None;
    }
    let (passed, failed, _) = run.counts();
    let mut parts = Vec::new();
    if passed > 0 {
        parts.push(format!("{passed} passed"));
    } else if pkg_ok > 0 && failed == 0 && run.diagnostics.is_empty() {
        parts.push(format!("{pkg_ok} packages ok"));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    } else if pkg_fail > 0 {
        parts.push(format!("{pkg_fail} packages failed"));
    }
    if !run.diagnostics.is_empty() {
        parts.push(format!("{} errors", run.diagnostics.len()));
    }
    if parts.is_empty() && pkg_ok > 0 {
        parts.push("go test ok".to_string());
    }
    if !parts.is_empty() {
        run.summary = Some(parts.join(", "));
    }
    Some(run)
}

fn parse_go_vet(exit: &Exit, stdout: &str, stderr: &str) -> Option<ParsedRun> {
    let re = Regex::new(r"^\s*(?:\./)?(\S+\.go):(\d+):(\d+):\s*(.*)$").ok()?;
    let mut run = ParsedRun::default();
    for line in stdout.lines().chain(stderr.lines()) {
        if let Some(caps) = re.captures(line) {
            run.diagnostics.push(Diagnostic {
                file: caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
                line: caps
                    .get(2)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(1),
                col: caps.get(3).and_then(|m| m.as_str().parse().ok()),
                severity: Severity::Error,
                code: None,
                message: caps.get(4).map(|m| m.as_str()).unwrap_or("").to_string(),
                tool: Some("go".to_string()),
            });
        }
    }
    if run.diagnostics.is_empty() {
        if exit.is_success() {
            run.summary = Some("go vet ok".to_string());
            return Some(run);
        }
        return None;
    }
    summarize_diagnostics(&mut run);
    Some(run)
}

fn parse_maven(exit: &Exit, stdout: &str, stderr: &str) -> Option<ParsedRun> {
    let re_err = Regex::new(r"^\[ERROR\]\s+([^\s:]+):\[(\d+),(\d+)\]\s*(.*)$").ok()?;
    let re_tests = Regex::new(
        r"Tests run:\s*(\d+),\s*Failures:\s*(\d+),\s*Errors:\s*(\d+),\s*Skipped:\s*(\d+)",
    )
    .ok()?;
    let mut run = ParsedRun::default();
    let mut total_run = 0usize;
    let mut total_fail = 0usize;
    let mut total_skip = 0usize;
    for line in stdout.lines().chain(stderr.lines()) {
        if let Some(caps) = re_err.captures(line.trim()) {
            run.diagnostics.push(Diagnostic {
                file: caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
                line: caps
                    .get(2)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(1),
                col: caps.get(3).and_then(|m| m.as_str().parse().ok()),
                severity: Severity::Error,
                code: None,
                message: caps.get(4).map(|m| m.as_str()).unwrap_or("").to_string(),
                tool: Some("maven".to_string()),
            });
        } else if let Some(caps) = re_tests.captures(line) {
            let r: usize = caps
                .get(1)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            let f: usize = caps
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            let e: usize = caps
                .get(3)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            let s: usize = caps
                .get(4)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            total_run += r;
            total_fail += f + e;
            total_skip += s;
        }
    }
    if total_run > 0 {
        let passed = total_run.saturating_sub(total_fail + total_skip);
        run.totals = Some(RunCounts {
            passed,
            failed: total_fail,
            ignored: total_skip,
        });
        run.summary = Some(format!(
            "{passed} passed, {total_fail} failed, {total_skip} skipped"
        ));
    }
    if run.diagnostics.is_empty() && run.totals.is_none() {
        if exit.is_success() {
            run.summary = Some("maven ok".to_string());
            return Some(run);
        }
        return None;
    }
    if run.summary.is_none() {
        summarize_diagnostics(&mut run);
    }
    Some(run)
}

fn parse_rspec(exit: &Exit, stdout: &str, stderr: &str) -> Option<ParsedRun> {
    let re_fail = Regex::new(r"^rspec\s+(\./)?([^\s:]+):(\d+)\s*#\s*(.*)$").ok()?;
    let re_sum = Regex::new(r"(\d+)\s+examples?,\s*(\d+)\s+failures?").ok()?;
    let mut run = ParsedRun::default();
    let mut examples = 0usize;
    let mut failures = 0usize;
    let mut saw_sum = false;
    for line in stdout.lines().chain(stderr.lines()) {
        if let Some(caps) = re_fail.captures(line.trim()) {
            let file = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            let line_num = caps.get(3).map(|m| m.as_str()).unwrap_or("1");
            let desc = caps.get(4).map(|m| m.as_str()).unwrap_or("");
            run.tests.push(TestResult {
                id: format!("{file}:{line_num}"),
                status: TestStatus::Fail,
                duration_ms: None,
                message: Some(desc.to_string()),
                location: Some(format!("{file}:{line_num}")),
            });
        } else if let Some(caps) = re_sum.captures(line) {
            saw_sum = true;
            examples = caps
                .get(1)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
            failures = caps
                .get(2)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0);
        }
    }
    if saw_sum {
        let passed = examples.saturating_sub(failures);
        run.totals = Some(RunCounts {
            passed,
            failed: failures,
            ignored: 0,
        });
        run.summary = Some(format!("{passed} passed, {failures} failed"));
    }
    if run.tests.is_empty() && !saw_sum {
        if exit.is_success() {
            run.summary = Some("rspec ok".to_string());
            return Some(run);
        }
        return None;
    }
    Some(run)
}

fn parse_rubocop(exit: &Exit, stdout: &str, stderr: &str) -> Option<ParsedRun> {
    let re =
        Regex::new(r"^([^\s:]+):(\d+):(\d+):\s*([CEWFR]):\s*(?:\[[^\]]+\]\s*)?([^:]+):\s*(.*)$")
            .ok()?;
    let mut run = ParsedRun::default();
    for line in stdout.lines().chain(stderr.lines()) {
        if let Some(caps) = re.captures(line.trim()) {
            let sev = match caps.get(4).map(|m| m.as_str()).unwrap_or("W") {
                "E" | "F" => Severity::Error,
                _ => Severity::Warn,
            };
            run.diagnostics.push(Diagnostic {
                file: caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
                line: caps
                    .get(2)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(1),
                col: caps.get(3).and_then(|m| m.as_str().parse().ok()),
                severity: sev,
                code: caps.get(5).map(|m| m.as_str().trim().to_string()),
                message: caps
                    .get(6)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
                tool: Some("rubocop".to_string()),
            });
        }
    }
    if run.diagnostics.is_empty() {
        if exit.is_success() {
            run.summary = Some("rubocop ok".to_string());
            return Some(run);
        }
        return None;
    }
    summarize_diagnostics(&mut run);
    Some(run)
}

fn parse_zig(exit: &Exit, stdout: &str, stderr: &str) -> Option<ParsedRun> {
    let re = Regex::new(r"^\s*([^\s:]+\.zig):(\d+):(\d+):\s*(error|warning|note):\s*(.*)$").ok()?;
    let mut run = ParsedRun::default();
    for line in stdout.lines().chain(stderr.lines()) {
        if let Some(caps) = re.captures(line) {
            let sev = match caps.get(4).map(|m| m.as_str()).unwrap_or("") {
                "error" => Severity::Error,
                "warning" => Severity::Warn,
                _ => Severity::Info,
            };
            run.diagnostics.push(Diagnostic {
                file: caps.get(1).map(|m| m.as_str()).unwrap_or("").to_string(),
                line: caps
                    .get(2)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(1),
                col: caps.get(3).and_then(|m| m.as_str().parse().ok()),
                severity: sev,
                code: None,
                message: caps.get(5).map(|m| m.as_str()).unwrap_or("").to_string(),
                tool: Some("zig".to_string()),
            });
        }
    }
    if run.diagnostics.is_empty() {
        if exit.is_success() {
            run.summary = Some("zig ok".to_string());
            return Some(run);
        }
        return None;
    }
    summarize_diagnostics(&mut run);
    Some(run)
}

fn summarize_diagnostics(run: &mut ParsedRun) {
    let errors = run
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    let warnings = run
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warn)
        .count();
    let mut parts = Vec::new();
    if errors > 0 {
        parts.push(count_label(errors, "error"));
    }
    if warnings > 0 {
        parts.push(count_label(warnings, "warning"));
    }
    if !parts.is_empty() {
        run.summary = Some(parts.join(", "));
    }
}

// ---------------------------------------------------------------------------
// Declarative TOML parsers (spec §10.1)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TomlParser {
    #[serde(default = "one")]
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub r#match: TomlMatch,
    #[serde(default)]
    pub inject: TomlInject,
    #[serde(default)]
    pub parse: TomlParse,
    #[serde(default)]
    pub map: TomlMap,
    #[serde(skip)]
    pub inject_args: Vec<String>,
}

fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TomlMatch {
    #[serde(default)]
    pub argv0: String,
    #[serde(default)]
    pub argv_contains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TomlInject {
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TomlParse {
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub success_exit: Vec<i32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TomlMap {
    #[serde(default)]
    pub test_result: TomlTestMap,
    #[serde(default)]
    pub status: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TomlTestMap {
    pub id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub location: String,
}

impl TomlParser {
    pub fn load(path: &Path) -> wxec_core::Result<Self> {
        let meta = std::fs::symlink_metadata(path).map_err(|e| {
            wxec_core::WErr::new("invalid_config", format!("parser {}: {e}", path.display()))
        })?;
        if !meta.file_type().is_file() {
            return Err(wxec_core::WErr::new(
                "invalid_config",
                format!("parser {}: not a regular file", path.display()),
            ));
        }
        if meta.len() > 64 * 1024 {
            return Err(wxec_core::WErr::new(
                "invalid_config",
                format!("parser {}: rule too large", path.display()),
            ));
        }
        let text = std::fs::read_to_string(path).map_err(|e| {
            wxec_core::WErr::new("invalid_config", format!("parser {}: {e}", path.display()))
        })?;
        let mut p: TomlParser = toml::from_str(&text).map_err(|e| {
            wxec_core::WErr::new("invalid_config", format!("parser {}: {e}", path.display()))
        })?;
        if p.name.is_empty() {
            p.name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
        }
        p.name = p.name.trim().to_string();
        if p.name.is_empty()
            || p.name.len() > 64
            || has_bad_token(&p.name)
            || p.name.contains('/')
            || p.name.contains('\\')
        {
            return Err(wxec_core::WErr::invalid_args(
                "invalid parser name",
                "use a short plain rule name",
            ));
        }
        if p.schema_version != 1 {
            return Err(wxec_core::WErr::invalid_args(
                format!(
                    "unsupported parser schema_version {} (expected 1)",
                    p.schema_version
                ),
                "schema_version=1",
            ));
        }
        p.r#match.argv0 = p.r#match.argv0.trim().to_string();
        if p.r#match.argv0.is_empty()
            || p.r#match.argv0.len() > 128
            || has_bad_token(&p.r#match.argv0)
        {
            return Err(wxec_core::WErr::invalid_args(
                "parser match.argv0 is required",
                "match.argv0=<command>",
            ));
        }
        if p.r#match.argv_contains.len() > 16 {
            return Err(wxec_core::WErr::invalid_args(
                "too many match.argv_contains entries",
                "limit argv_contains to 16 items",
            ));
        }
        for s in &mut p.r#match.argv_contains {
            *s = s.trim().to_string();
            if s.is_empty() || s.len() > 128 || has_bad_token(s) {
                return Err(wxec_core::WErr::invalid_args(
                    "invalid match.argv_contains entry",
                    "use short plain argv tokens",
                ));
            }
        }
        p.parse.format = p.parse.format.trim().to_ascii_lowercase();
        if p.parse.format.is_empty() {
            p.parse.format = "json".to_string();
        }
        if !matches!(p.parse.format.as_str(), "json" | "jsonl") {
            return Err(wxec_core::WErr::invalid_args(
                format!("unsupported parser parse.format '{}'", p.parse.format),
                "parse.format=json|jsonl",
            ));
        }
        if p.inject.args.len() > 16 {
            return Err(wxec_core::WErr::invalid_args(
                "too many inject args",
                "limit inject.args to 16 items",
            ));
        }
        for a in &p.inject.args {
            if a.is_empty() || a.len() > 256 || has_bad_token(a) {
                return Err(wxec_core::WErr::invalid_args(
                    "invalid inject arg",
                    "use short plain argv flags/values",
                ));
            }
        }
        if p.priority.abs() > 10_000 {
            p.priority = p.priority.clamp(-10_000, 10_000);
        }
        if p.map.status.len() > 64 {
            return Err(wxec_core::WErr::invalid_args(
                "too many map.status entries",
                "limit map.status to 64 items",
            ));
        }
        p.inject_args = p.inject.args.clone();
        Ok(p)
    }

    pub fn matches(&self, argv: &[String]) -> bool {
        if argv.is_empty() {
            return false;
        }
        if !self.r#match.argv0.is_empty() && base(&argv[0]) != self.r#match.argv0 {
            return false;
        }
        self.r#match
            .argv_contains
            .iter()
            .all(|needle| argv.iter().any(|a| a == needle))
    }

    pub fn parse(&self, stdout: &str, _stderr: &str) -> Option<ParsedRun> {
        let items = self.extract_items(stdout)?;
        let mut run = ParsedRun::default();
        for item in items {
            let id = json_path(&item, &self.map.test_result.id)
                .map(value_to_string)
                .unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            let status_raw = json_path(&item, &self.map.test_result.status)
                .map(value_to_string)
                .unwrap_or_default();
            let mapped = self
                .map
                .status
                .get(&status_raw)
                .cloned()
                .unwrap_or(status_raw);
            let status = parse_status(&mapped);
            let message = json_path(&item, &self.map.test_result.message)
                .map(value_to_string)
                .filter(|s| !s.is_empty());
            let location = json_path(&item, &self.map.test_result.location)
                .map(value_to_string)
                .filter(|s| !s.is_empty());
            run.tests.push(TestResult {
                id,
                status,
                duration_ms: None,
                message,
                location,
            });
        }
        if run.tests.is_empty() {
            return None;
        }
        let (passed, failed, skipped) = run.counts();
        let mut parts = Vec::new();
        if passed > 0 {
            parts.push(format!("{passed} passed"));
        }
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        if skipped > 0 {
            parts.push(format!("{skipped} skipped"));
        }
        run.summary = Some(parts.join(", "));
        Some(run)
    }

    fn extract_items(&self, stdout: &str) -> Option<Vec<Value>> {
        match self.parse.format.as_str() {
            "json" => {
                let root: Value = serde_json::from_str(stdout.trim()).ok()?;
                if let Some(arr) = root.as_array() {
                    Some(arr.clone())
                } else if let Some(arr) = root.get("tests").and_then(Value::as_array) {
                    Some(arr.clone())
                } else {
                    Some(vec![root])
                }
            }
            "jsonl" => {
                let mut items = Vec::new();
                for line in stdout.lines() {
                    let line = line.trim();
                    if line.starts_with('{') {
                        if let Ok(v) = serde_json::from_str::<Value>(line) {
                            items.push(v);
                        }
                    }
                }
                if items.is_empty() {
                    None
                } else {
                    Some(items)
                }
            }
            _ => None,
        }
    }
}

fn parse_status(s: &str) -> TestStatus {
    match s.to_ascii_lowercase().as_str() {
        "pass" | "passed" | "ok" | "success" => TestStatus::Pass,
        "fail" | "failed" | "failure" => TestStatus::Fail,
        "skip" | "skipped" | "ignored" => TestStatus::Skip,
        "xfail" => TestStatus::Xfail,
        "timeout" => TestStatus::Timeout,
        "error" => TestStatus::Error,
        _ => TestStatus::Error,
    }
}

fn json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let p = path.trim();
    if p.is_empty() {
        return None;
    }
    let p = p.strip_prefix('$').unwrap_or(p);
    let mut cur = value;
    for seg in p.split('.').filter(|s| !s.is_empty()) {
        let seg = seg.trim_start_matches('[').trim_end_matches(']');
        if let Ok(idx) = seg.parse::<usize>() {
            cur = cur.get(idx)?;
        } else {
            cur = cur.get(seg)?;
        }
    }
    Some(cur)
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn has_bad_token(s: &str) -> bool {
    s.chars().any(|c| c == '\0' || c.is_control())
}
