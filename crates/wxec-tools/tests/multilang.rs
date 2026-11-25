use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Value};
use wxec_core::config::Config;
use wxec_core::dispatcher::Dispatcher;
use wxec_core::parsers::ParserSet;
use wxec_core::types::{Exit, Severity, TestStatus, Tier};
use wxec_index::{syntax_errors, SymbolIndex};
use wxec_parsers::ParserRegistry;

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn setup_polyglot() -> (PathBuf, Dispatcher) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("wxec-multilang-{}-{}", std::process::id(), id));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let cache = root.with_extension("cache");
    let config_home = root.with_extension("config");
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_dir_all(&config_home);

    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);

    // Rust
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/engine.rs"),
        "pub struct Engine {\n    pub speed: u32,\n}\n\npub fn compute(x: u32) -> u32 {\n    x * 2\n}\n",
    )
    .unwrap();

    // Python
    std::fs::create_dir_all(root.join("app")).unwrap();
    std::fs::write(
        root.join("app/billing.py"),
        "class Invoice:\n    def total(self, amt: int) -> int:\n        return amt + 10\n\nasync def charge(user_id: str) -> bool:\n    return bool(user_id)\n",
    )
    .unwrap();

    // TypeScript, TSX, JavaScript
    std::fs::create_dir_all(root.join("web")).unwrap();
    std::fs::write(
        root.join("web/store.ts"),
        "export interface Item { id: string; }\nexport class Store {\n  count(): number { return 1; }\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("web/widget.tsx"),
        "export function Widget({ title }: { title: string }) {\n  return <h1>{title}</h1>;\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("web/legacy.js"),
        "function legacyInit(cfg) {\n  return cfg && cfg.enabled;\n}\n",
    )
    .unwrap();

    // Go
    std::fs::create_dir_all(root.join("pkg")).unwrap();
    std::fs::write(
        root.join("pkg/calc.go"),
        "package pkg\n\ntype Vector struct {\n\tX int\n}\n\nfunc Add(a int, b int) int {\n\treturn a + b\n}\n",
    )
    .unwrap();

    // Heuristic-outline languages: Java, Ruby, SQL, Bash
    std::fs::create_dir_all(root.join("jvm")).unwrap();
    std::fs::write(
        root.join("jvm/UserService.java"),
        "public class UserService {\n    public static void syncUsers() {\n    }\n}\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::write(
        root.join("scripts/tax.rb"),
        "module Billing\n  def calculate_tax(amount)\n    amount * 0.1\n  end\nend\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.join("db")).unwrap();
    std::fs::write(
        root.join("db/schema.sql"),
        "CREATE TABLE IF NOT EXISTS users (\n  id INT PRIMARY KEY\n);\nCREATE OR REPLACE VIEW active_users AS SELECT * FROM users;\n",
    )
    .unwrap();
    std::fs::write(
        root.join("scripts/deploy.sh"),
        "#!/usr/bin/env bash\ndeploy_app() {\n  echo deploying\n}\n",
    )
    .unwrap();

    let mut d = Dispatcher::new(
        Config::default(),
        &root,
        Arc::new(wxec_tools::registry()),
        Arc::new(ParserRegistry::new()),
    )
    .unwrap();
    d.set_session_tier("s", Tier::Execute).unwrap();
    (root, d)
}

fn call(d: &mut Dispatcher, tool: &str, args: Value) -> wxec_core::formatter::Rendered {
    d.call(tool, &args, "s")
}

#[test]
fn polyglot_symbol_extraction_and_editing() {
    let (_root, mut d) = setup_polyglot();

    // Batch-read symbols across Rust, Python, TypeScript, TSX, JavaScript, and Go in 1 turn
    let batch_reads = call(
        &mut d,
        "batch",
        json!({
            "calls": [
                {"tool": "read", "args": {"path": "engine.rs", "symbol": "compute"}},
                {"tool": "read", "args": {"path": "billing.py", "symbol": "charge"}},
                {"tool": "read", "args": {"path": "store.ts", "symbol": "Store"}},
                {"tool": "read", "args": {"path": "widget.tsx", "symbol": "Widget"}},
                {"tool": "read", "args": {"path": "legacy.js", "symbol": "legacyInit"}},
                {"tool": "read", "args": {"path": "calc.go", "symbol": "Add"}}
            ]
        }),
    );
    assert!(batch_reads.ok, "{}", batch_reads.text);
    for expected in [
        "x * 2",
        "return bool(user_id)",
        "count(): number",
        "<h1>{title}</h1>",
        "cfg.enabled",
        "return a + b",
    ] {
        assert!(
            batch_reads.text.contains(expected),
            "missing '{expected}' in:\n{}",
            batch_reads.text
        );
    }

    // Heuristic outline accuracy across Java, Ruby, SQL, and Bash (extract real names, not keywords)
    let idx = SymbolIndex::new();
    let java_syms = idx.symbols(
        std::path::Path::new("UserService.java"),
        "public class UserService {\n  private static final fun helper() {}\n}\n",
    );
    assert!(java_syms.heuristic);
    assert_eq!(java_syms.symbols[0].name, "UserService");

    let ruby_syms = idx.symbols(
        std::path::Path::new("tax.rb"),
        "module Billing\n  def calculate_tax(amount)\n  end\nend\n",
    );
    let ruby_names: Vec<&str> = ruby_syms.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(ruby_names, vec!["Billing", "calculate_tax"]);

    let sql_syms = idx.symbols(
        std::path::Path::new("schema.sql"),
        "CREATE TABLE IF NOT EXISTS users (id INT);\nCREATE OR REPLACE VIEW active_users AS SELECT 1;\n",
    );
    let sql_names: Vec<&str> = sql_syms.symbols.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(sql_names, vec!["users", "active_users"]);

    let sh_syms = idx.symbols(
        std::path::Path::new("deploy.sh"),
        "deploy_app() {\n  echo ok\n}\n",
    );
    assert_eq!(sh_syms.symbols[0].name, "deploy_app");
}

#[test]
fn polyglot_fast_ast_syntax_checks() {
    let p = std::path::Path::new;
    assert!(syntax_errors(p("a.rs"), "pub fn ok() -> i32 { 42 }").is_empty());
    assert!(!syntax_errors(p("a.rs"), "pub fn broken( { ").is_empty());

    assert!(syntax_errors(p("a.py"), "def ok(x: int) -> int:\n    return x + 1\n").is_empty());
    assert!(!syntax_errors(p("a.py"), "def broken(:\n    pass\n").is_empty());

    assert!(syntax_errors(p("a.ts"), "export const x: number = 1;").is_empty());
    assert!(!syntax_errors(p("a.ts"), "export const x: = ;").is_empty());

    assert!(syntax_errors(p("a.tsx"), "export const C = () => <div />;").is_empty());
    assert!(syntax_errors(p("a.go"), "package p\nfunc Ok() int { return 1 }\n").is_empty());
    assert!(!syntax_errors(p("a.go"), "package p\nfunc Broken( { }\n").is_empty());
}

#[test]
fn delta_compressed_reread_saves_tokens() {
    let (_root, mut d) = setup_polyglot();

    // Create a 60-line Python module
    let mut lines = Vec::new();
    for i in 1..=60 {
        lines.push(format!("def step_{i}(val: int) -> int: return val + {i}"));
    }
    let body = lines.join("\n") + "\n";
    let create = call(
        &mut d,
        "edit",
        json!({"path": "app/pipeline.py", "content": body}),
    );
    assert!(create.ok, "{}", create.text);

    // 1st read: full file
    let r1 = call(&mut d, "read", json!({"path": "app/pipeline.py"}));
    assert!(r1.ok, "{}", r1.text);
    assert!(r1.text.contains("60 lines"), "{}", r1.text);

    // Unchanged 2nd read: 1-line elision
    let r_unchanged = call(&mut d, "read", json!({"path": "app/pipeline.py"}));
    assert!(r_unchanged.ok, "{}", r_unchanged.text);
    assert!(
        r_unchanged.text.starts_with("unchanged "),
        "{}",
        r_unchanged.text
    );

    // External tool (e.g. formatter / another agent) modifies 1 line in the middle on disk
    let modified = body.replace(
        "def step_30(val: int) -> int: return val + 30",
        "def step_30(val: int) -> int: return val * 30",
    );
    std::fs::write(_root.join("app/pipeline.py"), modified).unwrap();

    // 3rd read after small edit: delta-compressed unified diff instead of all 60 lines
    let r_delta = call(&mut d, "read", json!({"path": "app/pipeline.py"}));
    assert!(r_delta.ok, "{}", r_delta.text);
    assert!(
        r_delta.text.starts_with("delta "),
        "expected delta-compressed reread, got:\n{}",
        r_delta.text
    );
    assert!(r_delta.text.contains("-def step_30"), "{}", r_delta.text);
    assert!(
        r_delta
            .text
            .contains("+def step_30(val: int) -> int: return val * 30"),
        "{}",
        r_delta.text
    );
    assert!(r_delta.text.contains("full: ~"), "{}", r_delta.text);
    assert!(
        r_delta.out_tokens < r1.out_tokens / 2,
        "delta tokens ({}) should be < 50% of full read ({})",
        r_delta.out_tokens,
        r1.out_tokens
    );

    // Force read bypasses delta and returns full file
    let r_force = call(
        &mut d,
        "read",
        json!({"path": "app/pipeline.py", "force": true}),
    );
    assert!(r_force.ok, "{}", r_force.text);
    assert!(r_force.text.contains("1: def step_1"), "{}", r_force.text);
}

#[test]
fn multi_type_code_testing_parsers_across_languages() {
    let reg = ParserRegistry::new();

    // 1. Type Checking (TypeScript tsc)
    let tsc_out =
        "web/store.ts(3,19): error TS2322: Type 'string' is not assignable to type 'number'.\n";
    let tsc_run = reg
        .parse(
            &["tsc".into(), "--noEmit".into()],
            &Exit::Code(2),
            tsc_out,
            "",
        )
        .unwrap();
    assert_eq!(tsc_run.diagnostics.len(), 1);
    assert_eq!(tsc_run.diagnostics[0].severity, Severity::Error);
    assert_eq!(tsc_run.diagnostics[0].code.as_deref(), Some("TS2322"));

    // 2. Linting (ESLint JSON + Ruff text)
    let eslint_json = json!([{
        "filePath": "web/legacy.js",
        "messages": [{"ruleId": "no-var", "severity": 2, "message": "Unexpected var.", "line": 1, "column": 1}]
    }])
    .to_string();
    let eslint_run = reg
        .parse(
            &["eslint".into(), "-f".into(), "json".into()],
            &Exit::Code(1),
            &eslint_json,
            "",
        )
        .unwrap();
    assert_eq!(eslint_run.diagnostics.len(), 1);
    assert_eq!(eslint_run.diagnostics[0].code.as_deref(), Some("no-var"));

    let ruff_out = json!([{
        "filename": "app/billing.py",
        "location": {"row": 2, "column": 5},
        "code": "F401",
        "message": "`os` imported but unused"
    }])
    .to_string();
    let ruff_run = reg
        .parse(
            &["ruff".into(), "check".into(), "--output-format=json".into()],
            &Exit::Code(1),
            &ruff_out,
            "",
        )
        .unwrap();
    assert_eq!(ruff_run.diagnostics.len(), 1);
    assert_eq!(ruff_run.diagnostics[0].code.as_deref(), Some("F401"));

    // 3. Unit & Subtest Runners (Go test -v subtests)
    let go_out = concat!(
        "=== RUN   TestCalc/add\n",
        "--- PASS: TestCalc/add (0.00s)\n",
        "=== RUN   TestCalc/sub\n",
        "--- FAIL: TestCalc/sub (0.00s)\n",
        "FAIL\tpkg\t0.012s\n",
    );
    let go_run = reg
        .parse(
            &["go".into(), "test".into(), "-v".into()],
            &Exit::Code(1),
            go_out,
            "",
        )
        .unwrap();
    assert_eq!(go_run.tests.len(), 2);
    assert_eq!(go_run.tests[0].status, TestStatus::Pass);
    assert_eq!(go_run.tests[1].status, TestStatus::Fail);

    // 4. Static Analysis (Go vet)
    let govet_err = "pkg/calc.go:8:2: unreachable code\n";
    let govet_run = reg
        .parse(&["go".into(), "vet".into()], &Exit::Code(1), "", govet_err)
        .unwrap();
    assert_eq!(govet_run.diagnostics.len(), 1);
    assert_eq!(govet_run.diagnostics[0].line, 8);

    // 5. Systems Language Diagnostics (Zig build/test)
    let zig_err = "src/main.zig:14:5: error: expected type 'u32', found 'i32'\n";
    let zig_run = reg
        .parse(&["zig".into(), "build".into()], &Exit::Code(1), "", zig_err)
        .unwrap();
    assert_eq!(zig_run.diagnostics.len(), 1);
    assert_eq!(zig_run.diagnostics[0].file, "src/main.zig");
    assert_eq!(zig_run.diagnostics[0].line, 14);
    assert_eq!(zig_run.diagnostics[0].severity, Severity::Error);

    // 6. Java Maven compiler + Surefire unit test counts
    let mvn_out = "[ERROR] src/Main.java:[10,4] ';' expected\nTests run: 5, Failures: 1, Errors: 0, Skipped: 1\n";
    let mvn_run = reg
        .parse(&["mvn".into(), "test".into()], &Exit::Code(1), mvn_out, "")
        .unwrap();
    assert_eq!(mvn_run.diagnostics.len(), 1);
    assert_eq!(mvn_run.diagnostics[0].line, 10);
    assert_eq!(mvn_run.total_counts(), (3, 1, 1));

    // 7. Ruby RSpec + RuboCop
    let rspec_out = "2 examples, 1 failure\nrspec ./spec/tax_spec.rb:14 # Tax calculates 10%\n";
    let rspec_run = reg
        .parse(
            &["bundle".into(), "exec".into(), "rspec".into()],
            &Exit::Code(1),
            rspec_out,
            "",
        )
        .unwrap();
    assert_eq!(rspec_run.total_counts(), (1, 1, 0));
    assert_eq!(rspec_run.tests[0].id, "spec/tax_spec.rb:14");

    let rubocop_out =
        "scripts/tax.rb:2:3: C: [Correctable] Style/NumericLiterals: Use underscores\n";
    let rubocop_run = reg
        .parse(&["rubocop".into()], &Exit::Code(1), rubocop_out, "")
        .unwrap();
    assert_eq!(rubocop_run.diagnostics.len(), 1);
    assert_eq!(
        rubocop_run.diagnostics[0].code.as_deref(),
        Some("Style/NumericLiterals")
    );
}

#[test]
fn intra_batch_zero_copy_handle_chaining() {
    let (root, mut d) = setup_polyglot();

    // In 1 single batch turn: read Rust file -> copy content to new file using ~prev -> expand ~step1
    let chained = call(
        &mut d,
        "batch",
        json!({
            "calls": [
                {"tool": "read", "args": {"path": "src/engine.rs"}},
                {"tool": "edit", "args": {"path": "src/engine_copy.rs", "content": "~prev"}}
            ]
        }),
    );
    assert!(chained.ok, "{}", chained.text);
    let orig = std::fs::read_to_string(root.join("src/engine.rs")).unwrap();
    let copied = std::fs::read_to_string(root.join("src/engine_copy.rs")).unwrap();
    assert_eq!(orig, copied);

    // Multi-file read across Rust, Python, TypeScript, and Go in a single call
    let multi = call(
        &mut d,
        "read",
        json!({
            "paths": ["src/engine.rs", "app/billing.py", "web/store.ts", "pkg/calc.go"],
            "lines": "1-3"
        }),
    );
    assert!(multi.ok, "{}", multi.text);
    assert!(multi.text.contains("4 files"), "{}", multi.text);
    assert!(multi.text.contains("pub struct Engine"), "{}", multi.text);
    assert!(multi.text.contains("class Invoice"), "{}", multi.text);
    assert!(
        multi.text.contains("export interface Item"),
        "{}",
        multi.text
    );
    assert!(multi.text.contains("package pkg"), "{}", multi.text);
}

#[test]
fn git_diff_elides_lockfiles_while_preserving_source_diff() {
    let (root, mut d) = setup_polyglot();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .env("HOME", &root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    assert!(git(&["init", "-q", "-b", "main"]));
    assert!(git(&["config", "user.email", "t@t"]));
    assert!(git(&["config", "user.name", "t"]));
    std::fs::write(root.join("Cargo.lock"), "v1\n").unwrap();
    assert!(git(&["add", "."]));
    assert!(git(&["commit", "-q", "-m", "init"]));

    // Modify both Cargo.lock (large noise) and src/engine.rs (actual code)
    let lock_lines: String = (1..=80).map(|i| format!("pkg-{i} = \"1.0.0\"\n")).collect();
    std::fs::write(root.join("Cargo.lock"), lock_lines).unwrap();
    std::fs::write(
        root.join("src/engine.rs"),
        "pub struct Engine {\n    pub speed: u32,\n}\n\npub fn compute(x: u32) -> u32 {\n    x * 3\n}\n",
    )
    .unwrap();

    let diff_res = call(&mut d, "git", json!({"op": "diff"}));
    assert!(diff_res.ok, "{}", diff_res.text);
    assert!(
        diff_res
            .text
            .contains("Cargo.lock: lockfile (+80 -1, elided)"),
        "expected lockfile elision in:\n{}",
        diff_res.text
    );
    assert!(
        diff_res.text.contains("+    x * 3"),
        "expected source diff to be preserved in:\n{}",
        diff_res.text
    );
    assert!(
        !diff_res.text.contains("pkg-10 ="),
        "lockfile body should be elided in:\n{}",
        diff_res.text
    );
}

#[test]
fn search_context_deduplicates_overlapping_lines() {
    let (_root, mut d) = setup_polyglot();
    let sample = "alpha\nneedle_one\nshared_middle\nneedle_two\nomega\n";
    let w = call(
        &mut d,
        "edit",
        json!({"path": "app/ctx_dedup.py", "content": sample}),
    );
    assert!(w.ok, "{}", w.text);

    let res = call(
        &mut d,
        "search",
        json!({"pattern": "needle_", "path": "app/ctx_dedup.py", "context": 1}),
    );
    assert!(res.ok, "{}", res.text);
    // shared_middle (line 3) is adjacent to both line 2 and line 4; it must appear only once!
    let shared_count = res.text.matches("3- shared_middle").count();
    assert_eq!(
        shared_count, 1,
        "expected deduplicated context:\n{}",
        res.text
    );
    // Match lines (2, 4) must not be duplicated as context lines
    assert!(!res.text.contains("2- needle_one"), "{}", res.text);
    assert!(!res.text.contains("4- needle_two"), "{}", res.text);
}

#[test]
fn symbol_not_found_surfaces_available_symbols_hint() {
    let (_root, mut d) = setup_polyglot();
    let miss = call(
        &mut d,
        "read",
        json!({"path": "app/billing.py", "symbol": "charg"}),
    );
    assert!(!miss.ok);
    assert!(miss.text.contains("symbol_not_found"), "{}", miss.text);
    assert!(
        miss.text
            .contains("available in app/billing.py: charge (L5)"),
        "expected fuzzy symbol suggestion in:\n{}",
        miss.text
    );

    // Outline includes start-end line spans for multi-line symbols
    let outline = call(
        &mut d,
        "read",
        json!({"path": "src/engine.rs", "outline": true}),
    );
    assert!(outline.ok, "{}", outline.text);
    assert!(
        outline.text.contains("1-3: struct Engine") && outline.text.contains("5-7: fn compute"),
        "expected span ranges in outline:\n{}",
        outline.text
    );

    // Multi-handle expand supports grep across multiple handles in 1 call
    let r_rs = call(
        &mut d,
        "read",
        json!({"path": "src/engine.rs", "budget": 64}),
    );
    let r_py = call(
        &mut d,
        "read",
        json!({"path": "app/billing.py", "budget": 64}),
    );
    assert!(r_rs.ok && r_py.ok);
    let h_rs = call(
        &mut d,
        "batch",
        json!({
            "calls": [
                {"tool": "read", "args": {"path": "src/engine.rs"}},
                {"tool": "read", "args": {"path": "app/billing.py"}},
                {"tool": "expand", "args": {"refs": ["~step1", "~step2"], "grep": "compute|charge"}}
            ]
        }),
    );
    assert!(h_rs.ok, "{}", h_rs.text);
    assert!(h_rs.text.contains("pub fn compute"), "{}", h_rs.text);
    assert!(h_rs.text.contains("async def charge"), "{}", h_rs.text);
    assert!(!h_rs.text.contains("class Invoice"), "{}", h_rs.text);

    // mode="symbol" search with case-insensitive prefilter across polyglot files
    let sym_search = call(
        &mut d,
        "search",
        json!({"pattern": "charge", "mode": "symbol"}),
    );
    assert!(sym_search.ok, "{}", sym_search.text);
    assert!(
        sym_search.text.contains("billing.py"),
        "{}",
        sym_search.text
    );
    assert!(sym_search.text.contains("charge"), "{}", sym_search.text);
}
