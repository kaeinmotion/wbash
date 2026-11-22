use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::{json, Value};

use wxec_core::args::parse_args;
use wxec_core::error::{Result, WErr};
use wxec_core::registry::{Tool, ToolContext};
use wxec_core::types::{Diagnostic, Severity, TestResult, Tier, ToolResult};

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
    #[serde(default = "all")]
    kind: String,
    #[serde(default = "dot")]
    path: String,
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
        "Run build/lint/type/test checks; normalized output."
    }
    fn min_tier(&self) -> Tier {
        Tier::Execute
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {"type": "string", "enum": ["all", "build", "lint", "types", "test"], "default": "all"},
                "path": {"type": "string", "default": "."}
            },
            "additionalProperties": false
        })
    }
    fn call(&self, args: &Value, ctx: &mut ToolContext<'_>) -> Result<ToolResult> {
        let a: CheckArgs = parse_args(
            args.clone(),
            "check",
            "pass kind=all|build|lint|types|test and optional path",
        )?;
        if !matches!(a.kind.as_str(), "all" | "build" | "lint" | "types" | "test") {
            return Err(WErr::invalid_args(
                format!("unknown kind '{}'", a.kind),
                "use all|build|lint|types|test",
            ));
        }
        let target = ctx.resolve(&a.path, true)?;
        let base = if target.is_dir() {
            target.clone()
        } else {
            target.parent().unwrap_or(ctx.root).to_path_buf()
        };
        let project = detect_root(ctx.root, &base).unwrap_or_else(|| base.clone());
        let cache_key = (project.clone(), a.kind.clone());
        let mut commands = {
            let cached = detect_cache()
                .lock()
                .ok()
                .and_then(|g| g.get(&cache_key).cloned());
            if let Some(cmds) = cached {
                cmds
            } else {
                let cmds = detect_commands(&project, &a.kind)?;
                if let Ok(mut g) = detect_cache().lock() {
                    g.insert(cache_key, cmds.clone());
                }
                cmds
            }
        };
        commands.dedup();
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
        for (label, argv) in &commands {
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
        if passed > 0 {
            header.push_str(&format!(", {passed} passed"));
        }
        if ignored > 0 {
            header.push_str(&format!(", {ignored} ignored"));
        }
        result.header = header;
        util::diagnostics_into(&mut result, diagnostics, ctx.root, ctx.handles)?;
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
            "go.mod",
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

    if project.join("pyproject.toml").exists() || project.join("setup.cfg").exists() {
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
                out.push(("npm lint".to_string(), s(&["npm", "run", "lint"])));
            }
        }
        if want("test") {
            if let Some(vitest) = bin("vitest") {
                out.push(("vitest".to_string(), vec![vitest, "run".to_string()]));
            } else if let Some(jest) = bin("jest") {
                out.push(("jest".to_string(), vec![jest]));
            } else if has_script(project, "test") {
                out.push(("npm test".to_string(), s(&["npm", "test"])));
            }
        }
        if want("build") && has_script(project, "build") {
            out.push(("npm build".to_string(), s(&["npm", "run", "build"])));
        }
    }

    let mut seen = BTreeSet::new();
    out.retain(|(_, argv)| seen.insert(argv.join("\u{0}")));
    Ok(out)
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

pub fn register(out: &mut Vec<std::sync::Arc<dyn Tool>>) {
    out.push(std::sync::Arc::new(CheckTool));
}

#[cfg(test)]
mod tests {
    use super::detect_commands;

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
}
