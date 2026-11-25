use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::{json, Value};

use wxec_core::args::parse_args;
use wxec_core::error::{Result, WErr};
use wxec_core::registry::{Tool, ToolContext};
use wxec_core::types::{Diagnostic, HandleRef, Severity, TestResult, Tier, ToolResult};

use crate::run::execute_with_parsers;
use crate::util;

type DetectCacheMap = HashMap<(PathBuf, String), Vec<(String, Vec<String>)>>;

/// Session-scoped cache of detected commands keyed by (project_root, kind).
/// Re-detecting on every `check` call costs 6–10 syscalls; this avoids them.
static DETECT_CACHE: OnceLock<Mutex<DetectCacheMap>> = OnceLock::new();

fn detect_cache() -> &'static Mutex<DetectCacheMap> {
    DETECT_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct CheckTool;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckArgs {
    #[serde(default = "all", alias = "type", alias = "target")]
    kind: String,
    #[serde(
        default = "dot",
        alias = "dir",
        alias = "working_dir",
        alias = "directory",
        alias = "folder"
    )]
    path: String,
    #[serde(
        default,
        deserialize_with = "wxec_core::args::deserialize_argv_opt",
        alias = "filter",
        alias = "test",
        alias = "name",
        alias = "extra_args"
    )]
    args: Option<Vec<String>>,
}

fn all() -> String {
    "all".to_string()
}
fn dot() -> String {
    ".".to_string()
}

impl Tool for CheckTool {
    fn name(&self) -> &'static str {
        "check"
    }
    fn description(&self) -> &'static str {
        "Run build, lint or test checks."
    }
    fn min_tier(&self) -> Tier {
        Tier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {"type": "string", "enum": ["all", "build", "lint", "types", "test"]},
                "path": {"type": "string"},
                "args": {"type": ["string", "array"]},
                "filter": {"type": "string"}
            },
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: CheckArgs = parse_args(
            args.clone(),
            "check",
            "pass kind=all|build|lint|types|test and optional path",
        )?;
        let kind = match a.kind.to_ascii_lowercase().as_str() {
            "all" | "check" => "all".to_string(),
            "build" | "compile" => "build".to_string(),
            "lint" | "lints" | "clippy" | "fmt" => "lint".to_string(),
            "types" | "type" | "typecheck" => "types".to_string(),
            "test" | "tests" => "test".to_string(),
            other => {
                return Err(WErr::invalid_args(
                    format!("unknown kind '{other}'"),
                    "use all|build|lint|types|test",
                ));
            }
        };
        let target = ctx.resolve(&a.path, true)?;
        let base = if target.is_dir() {
            target.clone()
        } else {
            target.parent().unwrap_or(ctx.root).to_path_buf()
        };
        let project = detect_root(ctx.root, &base).unwrap_or_else(|| base.clone());
        let cache_key = (project.clone(), kind.clone());
        let mut commands = {
            let cached = detect_cache()
                .lock()
                .ok()
                .and_then(|g| g.get(&cache_key).cloned());
            if let Some(cmds) = cached {
                cmds
            } else {
                let cmds = detect_commands(&project, &kind)?;
                if let Ok(mut g) = detect_cache().lock() {
                    g.insert(cache_key, cmds.clone());
                }
                cmds
            }
        };
        commands.dedup();
        if let Some(user_args) = &a.args {
            if !user_args.is_empty() {
                for (label, argv) in &mut commands {
                    if kind != "all" || label.contains("test") || label.contains("pytest") {
                        apply_test_filter(label, argv, user_args);
                    }
                }
            }
        }
        if commands.is_empty() {
            return Err(WErr::with_hint(
                "no_checker_detected",
                format!("no checker detected under {}", ctx.rel(&project)),
                "add a Cargo.toml, package.json, pyproject.toml/setup.cfg or go.mod, or run the tool directly with `w run`",
            ));
        }

        let mut result = ToolResult::new("check", String::new());
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let mut tests: Vec<TestResult> = Vec::new();
        let mut all_ok = true;
        let mut ran = 0usize;
        let mut missing = 0usize;
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut ignored = 0usize;
        let verbose = ctx.config.output.verbosity == "verbose";
        let mut build_failed = false;
        for (label, argv) in &commands {
            let is_test_cmd = label.contains("test")
                || label.contains("pytest")
                || label.contains("vitest")
                || label.contains("jest")
                || label.contains("rspec");
            if build_failed && is_test_cmd {
                result.note(format!("skipped {label} (build failed)"));
                continue;
            }
            let run = match execute_with_parsers(
                ctx,
                argv,
                &project,
                ctx.config.exec.timeout_ms,
                None,
                None,
                true,
            ) {
                Ok(r) => r,
                Err(e) if e.code == "exec_not_found" => {
                    result.note(format!("missing {label}"));
                    missing += 1;
                    continue;
                }
                Err(e) => return Err(e),
            };
            ran += 1;
            all_ok &= run.output.exit.is_success();
            result.meta.raw_bytes = Some(
                result.meta.raw_bytes.unwrap_or(0)
                    + run.output.stdout.len() as u64
                    + run.output.stderr.len() as u64,
            );
            if run.output.timed_out {
                result.note(format!("timeout {label}"));
            }
            if let Some(p) = run.parsed {
                let (pp, pf, pi) = p.total_counts();
                passed += pp;
                failed += pf;
                ignored += pi;
                let has_errors = p.diagnostics.iter().any(|d| d.severity == Severity::Error);
                if !run.output.exit.is_success()
                    && has_errors
                    && (label.contains("build")
                        || label.contains("check")
                        || label.contains("compile")
                        || label.contains("tsc"))
                {
                    build_failed = true;
                }
                all_ok &= run.output.exit.is_success() && !has_errors && pf == 0;
                let has_structured = !p.diagnostics.is_empty()
                    || !p.tests.is_empty()
                    || pf > 0
                    || p.summary
                        .as_ref()
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false);
                diagnostics.extend(p.diagnostics.clone());
                tests.extend(p.tests.clone());
                if run.output.exit.is_success() {
                    if verbose {
                        if let Some(summary) = p.summary.as_ref().filter(|s| !s.trim().is_empty()) {
                            result.note(format!("{label}: {summary}"));
                        } else {
                            result.note(format!("{label}: ok"));
                        }
                    }
                } else if !has_structured {
                    result.note(format!("{label} parse_miss"));
                    let raw = run.output.combined();
                    util::compact_tail(&mut result, &raw, label, ctx.handles, true)?;
                } else if verbose {
                    if let Some(summary) = p.summary.as_ref().filter(|s| !s.trim().is_empty()) {
                        result.note(format!("{label}: {summary}"));
                    }
                }
            } else if run.output.exit.is_success() {
                if verbose {
                    result.note(format!("{label}: ok"));
                }
            } else {
                result.note(format!("{label} parse_miss"));
                let raw = run.output.combined();
                util::compact_tail(&mut result, &raw, label, ctx.handles, true)?;
            }
            if run.output.truncated {
                result.note(format!("{label} output_truncated"));
            }
        }
        if ran == 0 {
            return Err(WErr::with_hint(
                "no_checker_detected",
                "detected checkers are not installed",
                "install the toolchain or run the checker directly with `w run`",
            ));
        }
        let mut seen_diag = std::collections::HashSet::new();
        diagnostics.retain(|d| {
            seen_diag.insert((
                d.file.clone(),
                d.line,
                d.col,
                d.severity.as_str(),
                d.message.clone(),
            ))
        });
        let mut seen_tests = std::collections::HashSet::new();
        tests.retain(|t| seen_tests.insert(t.id.clone()));
        let errors = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let warnings = diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Warn)
            .count();
        let ok = all_ok && errors == 0 && failed == 0 && missing == 0;
        let mut header = if errors == 0 && failed == 0 && ok {
            if warnings == 0 {
                "ok".to_string()
            } else {
                format!("ok, {}", util::plural(warnings, "warning"))
            }
        } else {
            let mut parts = Vec::new();
            if !ok && errors == 0 && failed == 0 && missing == 0 {
                parts.push("failed".to_string());
            }
            if errors > 0 {
                parts.push(util::plural(errors, "error"));
            }
            if warnings > 0 {
                parts.push(util::plural(warnings, "warning"));
            }
            if failed > 0 {
                parts.push(util::plural(failed, "failed test"));
            }
            if missing > 0 {
                parts.push(format!("{} missing", missing));
            }
            parts.join(", ")
        };
        if failed > 0 && !header.contains("failed") {
            header.push_str(&format!(", {failed} failed"));
        }
        if errors == 0 && failed == 0 {
            if passed > 0 {
                header.push_str(&format!(", {passed} passed"));
            }
            if ignored > 0 {
                header.push_str(&format!(", {ignored} ignored"));
            }
        }
        result.header = header;
        let filtered_diags = if errors > 0 && ctx.config.output.verbosity != "verbose" {
            if warnings > 0 {
                result.note(format!("{warnings} warnings omitted while errors exist"));
            }
            let err_only: Vec<_> = diagnostics
                .into_iter()
                .filter(|d| d.severity == Severity::Error)
                .collect();
            if err_only.len() > 8 {
                let count = err_only.len();
                let kept: Vec<_> = err_only.iter().take(5).cloned().collect();
                let omitted: Vec<_> = err_only.iter().skip(5).cloned().collect();
                let omitted_text: Vec<String> = omitted.iter().map(|d| d.render()).collect();
                if let Ok(href) =
                    ctx.handles
                        .put("flood", omitted_text.join("\n").as_bytes(), "text")
                {
                    result.note(format!("...+{} errors ({})", count - 5, href.id));
                    result.handle(HandleRef {
                        id: href.id,
                        kind: href.kind,
                        bytes: href.bytes,
                        hash: href.hash,
                    });
                }
                kept
            } else {
                err_only
            }
        } else if warnings > 5 && errors == 0 && ctx.config.output.verbosity != "verbose" {
            let mut kept = Vec::new();
            let mut omitted = Vec::new();
            for d in diagnostics {
                if kept.len() < 3 {
                    kept.push(d);
                } else {
                    omitted.push(d);
                }
            }
            if !omitted.is_empty() {
                let omitted_text: Vec<String> = omitted.iter().map(|d| d.render()).collect();
                if let Ok(href) =
                    ctx.handles
                        .put("flood", omitted_text.join("\n").as_bytes(), "text")
                {
                    result.note(format!("...+{} warnings ({})", omitted.len(), href.id));
                    result.handle(HandleRef {
                        id: href.id,
                        kind: href.kind,
                        bytes: href.bytes,
                        hash: href.hash,
                    });
                }
            }
            kept
        } else {
            diagnostics
        };
        util::diagnostics_into(&mut result, filtered_diags, ctx.root, ctx.handles)?;
        util::tests_into(&mut result, &tests, true);
        result.meta.ok = ok;
        result.meta.counts.insert("errors".into(), errors as i64);
        result
            .meta
            .counts
            .insert("warnings".into(), warnings as i64);
        if failed > 0 {
            result.meta.counts.insert("failed".into(), failed as i64);
        }
        if passed > 0 {
            result.meta.counts.insert("passed".into(), passed as i64);
        }
        if ignored > 0 {
            result.meta.counts.insert("ignored".into(), ignored as i64);
        }

        Ok(result)
    }
}

fn detect_root(root: &Path, start: &Path) -> Option<PathBuf> {
    let mut cur = start.to_path_buf();
    loop {
        for marker in [
            "Cargo.toml",
            "package.json",
            "pyproject.toml",
            "setup.cfg",
            "requirements.txt",
            "go.mod",
            "build.zig",
            "Gemfile",
            "pom.xml",
            "build.gradle",
            "build.gradle.kts",
        ] {
            if cur.join(marker).exists() {
                return Some(cur);
            }
        }
        if cur == root {
            return None;
        }
        if !cur.pop() {
            return None;
        }
        if !cur.starts_with(root) {
            return None;
        }
    }
}

fn detect_commands(project: &Path, kind: &str) -> Result<Vec<(String, Vec<String>)>> {
    let mut out = Vec::new();
    let want = |k: &str| kind == "all" || kind == k;
    let s = |parts: &[&str]| parts.iter().map(|p| p.to_string()).collect::<Vec<_>>();

    if project.join("Cargo.toml").exists() {
        if (want("build") || want("types")) && kind != "all" {
            out.push(("cargo check".to_string(), s(&["cargo", "check"])));
        }
        if want("lint") {
            out.push((
                "cargo clippy".to_string(),
                s(&["cargo", "clippy", "--all-targets"]),
            ));
        }
        if want("test") {
            out.push(("cargo test".to_string(), s(&["cargo", "test"])));
        }
    }

    if project.join("go.mod").exists() {
        let lint = want("lint");
        let test = want("test");
        if (want("build") || want("types")) && !(kind == "all" && (lint || test)) {
            out.push(("go build".to_string(), s(&["go", "build", "./..."])));
        }
        if lint {
            out.push(("go vet".to_string(), s(&["go", "vet", "./..."])));
        }
        if test {
            out.push(("go test".to_string(), s(&["go", "test", "./..."])));
        }
    }

    if project.join("pyproject.toml").exists()
        || project.join("setup.cfg").exists()
        || project.join("requirements.txt").exists()
        || project.join("pytest.ini").exists()
    {
        if want("test") {
            if let Some(bin) = find_executable("pytest", project) {
                out.push(("pytest".to_string(), vec![bin, "-q".to_string()]));
            }
        }
        if want("lint") {
            if let Some(bin) = find_executable("ruff", project) {
                out.push((
                    "ruff".to_string(),
                    vec![bin, "check".to_string(), ".".to_string()],
                ));
            }
        }
        if want("types") {
            if let Some(bin) = find_executable("mypy", project) {
                out.push(("mypy".to_string(), vec![bin, ".".to_string()]));
            }
        }
    }

    if project.join("package.json").exists() {
        let runner = detect_js_runner(project);
        let bin = |name: &str| find_executable(name, project);
        if want("types") && project.join("tsconfig.json").exists() {
            if let Some(tsc) = bin("tsc") {
                out.push(("tsc".to_string(), vec![tsc, "--noEmit".to_string()]));
            }
        }
        if want("lint") {
            if let Some(eslint) = bin("eslint") {
                out.push(("eslint".to_string(), vec![eslint, ".".to_string()]));
            } else if has_script(project, "lint") {
                let cmd = if runner == "npm" {
                    vec!["npm".to_string(), "run".to_string(), "lint".to_string()]
                } else {
                    vec![runner.to_string(), "run".to_string(), "lint".to_string()]
                };
                out.push((format!("{runner} lint"), cmd));
            }
        }
        if want("test") {
            if let Some(vitest) = bin("vitest") {
                out.push(("vitest".to_string(), vec![vitest, "run".to_string()]));
            } else if let Some(jest) = bin("jest") {
                out.push(("jest".to_string(), vec![jest]));
            } else if has_script(project, "test") {
                let cmd = if runner == "npm" {
                    vec!["npm".to_string(), "test".to_string()]
                } else {
                    vec![runner.to_string(), "test".to_string()]
                };
                out.push((format!("{runner} test"), cmd));
            }
        }
        if want("build") && has_script(project, "build") {
            let cmd = if runner == "npm" {
                vec!["npm".to_string(), "run".to_string(), "build".to_string()]
            } else {
                vec![runner.to_string(), "run".to_string(), "build".to_string()]
            };
            out.push((format!("{runner} build"), cmd));
        }
    }

    if project.join("build.zig").exists() {
        if want("build") {
            out.push(("zig build".to_string(), s(&["zig", "build"])));
        }
        if want("test") {
            out.push(("zig test".to_string(), s(&["zig", "build", "test"])));
        }
    }

    if project.join("Gemfile").exists() {
        if want("test") {
            out.push(("rspec".to_string(), s(&["bundle", "exec", "rspec"])));
        }
        if want("lint") {
            out.push(("rubocop".to_string(), s(&["bundle", "exec", "rubocop"])));
        }
    }

    if project.join("pom.xml").exists() {
        if want("build") {
            out.push(("mvn compile".to_string(), s(&["mvn", "-q", "compile"])));
        }
        if want("test") {
            out.push(("mvn test".to_string(), s(&["mvn", "-q", "test"])));
        }
    }

    if project.join("build.gradle").exists() || project.join("build.gradle.kts").exists() {
        let gradle = if project.join("gradlew").exists() {
            "./gradlew"
        } else {
            "gradle"
        };
        if want("test") {
            out.push(("gradle test".to_string(), s(&[gradle, "test"])));
        }
    }

    let mut seen = BTreeSet::new();
    out.retain(|(_, argv)| seen.insert(argv.join("\u{0}")));
    Ok(out)
}

fn detect_js_runner(project: &Path) -> &'static str {
    if project.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if project.join("bun.lockb").exists() || project.join("bun.lock").exists() {
        "bun"
    } else if project.join("yarn.lock").exists() {
        "yarn"
    } else {
        "npm"
    }
}

fn has_script(project: &Path, name: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(project.join("package.json")) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return false;
    };
    v.get("scripts").and_then(|s| s.get(name)).is_some()
}

fn find_executable(name: &str, project: &Path) -> Option<String> {
    let local = [
        project.join("node_modules").join(".bin").join(name),
        project.join(".venv").join("bin").join(name),
        project.join("venv").join("bin").join(name),
    ];
    for p in local {
        if p.is_file() {
            return Some(p.to_string_lossy().to_string());
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

fn apply_test_filter(label: &str, argv: &mut Vec<String>, user_args: &[String]) {
    if user_args.len() == 1 && !user_args[0].starts_with('-') {
        let f = &user_args[0];
        if label == "go test" {
            argv.push("-run".to_string());
            argv.push(f.clone());
            return;
        }
        if label == "pytest" && !f.ends_with(".py") && !f.contains('/') {
            argv.push("-k".to_string());
            argv.push(f.clone());
            return;
        }
        if label == "mvn test" {
            argv.push(format!("-Dtest={f}"));
            return;
        }
        if label == "gradle test" {
            argv.push("--tests".to_string());
            argv.push(f.clone());
            return;
        }
    }
    argv.extend(user_args.iter().cloned());
}

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(CheckTool));
}

#[cfg(test)]
mod tests {
    use super::{apply_test_filter, detect_commands};

    #[test]
    fn polyglot_test_filter_translation() {
        let mut go = vec!["go".into(), "test".into(), "./...".into()];
        apply_test_filter("go test", &mut go, &["TestCalc".into()]);
        assert_eq!(go, vec!["go", "test", "./...", "-run", "TestCalc"]);

        let mut py = vec!["pytest".into(), "-q".into()];
        apply_test_filter("pytest", &mut py, &["test_billing".into()]);
        assert_eq!(py, vec!["pytest", "-q", "-k", "test_billing"]);

        let mut py_file = vec!["pytest".into(), "-q".into()];
        apply_test_filter("pytest", &mut py_file, &["tests/test_a.py".into()]);
        assert_eq!(py_file, vec!["pytest", "-q", "tests/test_a.py"]);

        let mut mvn = vec!["mvn".into(), "-q".into(), "test".into()];
        apply_test_filter("mvn test", &mut mvn, &["UserServiceTest".into()]);
        assert_eq!(mvn, vec!["mvn", "-q", "test", "-Dtest=UserServiceTest"]);

        let mut gradle = vec!["gradle".into(), "test".into()];
        apply_test_filter("gradle test", &mut gradle, &["UserTest".into()]);
        assert_eq!(gradle, vec!["gradle", "test", "--tests", "UserTest"]);
    }

    #[test]
    fn rust_all_skips_redundant_check() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
        let cmds = detect_commands(dir.path(), "all").unwrap();
        let labels: Vec<_> = cmds.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, vec!["cargo clippy", "cargo test"]);
    }

    #[test]
    fn rust_types_runs_check() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
        let cmds = detect_commands(dir.path(), "types").unwrap();
        let labels: Vec<_> = cmds.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, vec!["cargo check"]);
    }

    #[test]
    fn go_all_skips_redundant_build() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("go.mod"), "module x\ngo 1.22\n").unwrap();
        let cmds = detect_commands(dir.path(), "all").unwrap();
        let labels: Vec<_> = cmds.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, vec!["go vet", "go test"]);
    }

    #[test]
    fn js_package_manager_detection() {
        use super::detect_js_runner;

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"test": "echo test", "lint": "echo lint", "build": "echo build"}}"#,
        )
        .unwrap();

        std::fs::write(dir.path().join("pnpm-lock.yaml"), "lockfile").unwrap();
        assert_eq!(detect_js_runner(dir.path()), "pnpm");
        let cmds = detect_commands(dir.path(), "all").unwrap();
        let labels: Vec<_> = cmds.iter().map(|(l, _)| l.as_str()).collect();
        assert!(labels.contains(&"pnpm lint"));
        assert!(labels.contains(&"pnpm test"));
        assert!(labels.contains(&"pnpm build"));

        std::fs::remove_file(dir.path().join("pnpm-lock.yaml")).unwrap();
        std::fs::write(dir.path().join("bun.lockb"), "lockfile").unwrap();
        assert_eq!(detect_js_runner(dir.path()), "bun");

        std::fs::remove_file(dir.path().join("bun.lockb")).unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "lockfile").unwrap();
        assert_eq!(detect_js_runner(dir.path()), "yarn");

        std::fs::remove_file(dir.path().join("yarn.lock")).unwrap();
        assert_eq!(detect_js_runner(dir.path()), "npm");
    }
}
