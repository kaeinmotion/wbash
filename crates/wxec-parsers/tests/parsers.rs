use serde_json::json;
use wxec_core::parsers::ParserSet;
use wxec_core::types::{Exit, Severity, TestStatus};
use wxec_parsers::{ParserRegistry, TomlParser};

fn registry() -> ParserRegistry {
    ParserRegistry::new()
}

#[test]
fn cargo_compiler_and_libtest() {
    let reg = registry();
    let argv = vec!["cargo".to_string(), "test".to_string()];
    let stdout = r#"{"reason":"compiler-message","message":{"level":"error","message":"no method","code":{"code":"E0599"},"spans":[{"file_name":"src/a.rs","line_start":3,"column_start":7,"is_primary":true}]}}
test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
test works ... ok
test fails ... FAILED
---- fails stdout ----
assertion failed: left == right
failures:
"#;
    let run = reg.parse(&argv, &Exit::Code(101), stdout, "").unwrap();
    assert_eq!(run.parser.as_deref(), Some("cargo"));
    assert_eq!(run.tests.len(), 2);
    assert_eq!(run.tests[0].status, TestStatus::Pass);
    assert_eq!(run.tests[1].status, TestStatus::Fail);
    assert_eq!(
        run.tests[1].message.as_deref(),
        Some("assertion failed: left == right")
    );
    assert_eq!(run.diagnostics.len(), 1);
    assert_eq!(run.diagnostics[0].severity, Severity::Error);
    assert_eq!(run.diagnostics[0].code.as_deref(), Some("E0599"));
}

#[test]
fn cargo_libtest_assertion_preserves_left_right() {
    let reg = registry();
    let argv = vec!["cargo".to_string(), "test".to_string()];
    let stdout = r#"{"reason":"compiler-artifact","package_id":"x 0.1.0"}
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
test fails ... FAILED
---- fails stdout ----
thread 'fails' panicked at src/lib.rs:4:5:
assertion `left == right` failed
  left: 1
 right: 2
note: run with `RUST_BACKTRACE=1`
failures:
"#;
    let run = reg.parse(&argv, &Exit::Code(101), stdout, "").unwrap();
    let msg = run.tests[0].message.as_deref().unwrap_or("");
    assert!(msg.contains("assertion"), "{msg}");
    assert!(msg.contains("left: 1"), "{msg}");
    assert!(msg.contains("right: 2"), "{msg}");
}

#[test]
fn pytest_text() {
    let reg = registry();
    let argv = vec!["pytest".to_string(), "-q".to_string()];
    let out = "FAILED tests/test_a.py::test_x - assert 1 == 2\n============ 1 failed, 2 passed in 0.12s ============\n";
    let run = reg.parse(&argv, &Exit::Code(1), out, "").unwrap();
    assert_eq!(run.tests.len(), 1);
    assert_eq!(run.tests[0].id, "tests/test_a.py::test_x");
    assert_eq!(run.summary.as_deref(), Some("1 failed, 2 passed"));
}

#[test]
fn jest_and_vitest_json() {
    let reg = registry();
    let jest = json!({
        "numPassedTests": 1,
        "testResults": [{
            "name": "/repo/src/a.test.ts",
            "assertionResults": [
                {"fullName": "a passes", "status": "passed"},
                {"fullName": "a fails", "status": "failed", "failureMessages": ["Expected 1\n    at x"]}
            ]
        }]
    })
    .to_string();
    let argv = vec!["jest".to_string(), "--json".to_string()];
    let run = reg.parse(&argv, &Exit::Code(1), &jest, "").unwrap();
    assert_eq!(run.tests.len(), 2);
    assert_eq!(run.tests[1].status, TestStatus::Fail);
    assert_eq!(run.tests[1].message.as_deref(), Some("Expected 1"));

    let vitest = jest.clone();
    let argv = vec!["vitest".to_string(), "--reporter=json".to_string()];
    let run = reg.parse(&argv, &Exit::Code(1), &vitest, "").unwrap();
    assert_eq!(run.tests.len(), 2);
}

#[test]
fn tsc_eslint_ruff_go() {
    let reg = registry();
    let tsc = "src/a.ts(3,5): error TS2322: Type 'string' is not assignable\nsrc/b.ts:9:1 - warning TS9999: hmm\n";
    let run = reg
        .parse(&["tsc".to_string()], &Exit::Code(2), tsc, "")
        .unwrap();
    assert_eq!(run.diagnostics.len(), 2);
    assert_eq!(run.diagnostics[0].file, "src/a.ts");
    assert_eq!(run.diagnostics[1].severity, Severity::Warn);

    let eslint = json!([{
        "filePath": "/repo/src/a.js",
        "messages": [{"line": 4, "column": 2, "severity": 2, "ruleId": "no-unused-vars", "message": "unused"}]
    }])
    .to_string();
    let run = reg
        .parse(
            &["eslint".to_string(), "-f".to_string(), "json".to_string()],
            &Exit::Code(1),
            &eslint,
            "",
        )
        .unwrap();
    assert_eq!(run.diagnostics[0].file, "/repo/src/a.js");
    assert_eq!(run.diagnostics[0].code.as_deref(), Some("no-unused-vars"));

    let ruff = json!([{"filename": "src/a.py", "location": {"row": 7, "column": 3}, "code": "F401", "message": "unused import"}]).to_string();
    let run = reg
        .parse(
            &["ruff".to_string(), "check".to_string()],
            &Exit::Code(1),
            &ruff,
            "",
        )
        .unwrap();
    assert_eq!(run.diagnostics[0].line, 7);
    assert_eq!(run.diagnostics[0].severity, Severity::Warn);

    let go = "--- FAIL: TestA (0.00s)\n--- PASS: TestB (0.00s)\n./a.go:12:3: undefined: x\n";
    let run = reg
        .parse(
            &["go".to_string(), "test".to_string(), "./...".to_string()],
            &Exit::Code(1),
            go,
            "",
        )
        .unwrap();
    assert_eq!(run.tests.len(), 2);
    assert_eq!(run.diagnostics[0].file, "a.go");
}

#[test]
fn toml_parser_and_injection() {
    let dir = std::env::temp_dir().join(format!("wxec-parser-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("demo.toml");
    std::fs::write(
        &path,
        r#"
schema_version = 1
name = "demo"
priority = 100
[match]
argv0 = "demo"
argv_contains = ["run"]
[inject]
args = ["--json"]
[parse]
format = "jsonl"
[map.test_result]
id = "$.name"
status = "$.state"
message = "$.msg"
[map.status]
ok = "pass"
bad = "fail"
"#,
    )
    .unwrap();
    let parser = TomlParser::load(&path).unwrap();
    let reg = registry().with_toml(parser);
    let argv = vec!["demo".to_string(), "run".to_string()];
    assert_eq!(reg.inject_args(&argv), vec!["demo", "run", "--json"]);
    let sample = "{\"name\":\"one\",\"state\":\"ok\"}\n{\"name\":\"two\",\"state\":\"bad\",\"msg\":\"boom\"}\n";
    let run = reg.parse(&argv, &Exit::Code(1), sample, "").unwrap();
    assert_eq!(run.parser.as_deref(), Some("demo"));
    assert_eq!(run.tests[0].status, TestStatus::Pass);
    assert_eq!(run.tests[1].status, TestStatus::Fail);
    assert_eq!(run.tests[1].message.as_deref(), Some("boom"));
}

#[test]
fn cargo_injects_before_separator() {
    let reg = registry();
    let argv = vec![
        "cargo".to_string(),
        "clippy".to_string(),
        "-q".to_string(),
        "--".to_string(),
        "-D".to_string(),
        "warnings".to_string(),
    ];
    let injected = reg.inject_args(&argv);
    assert_eq!(
        injected,
        vec![
            "cargo",
            "clippy",
            "-q",
            "--message-format=json",
            "--",
            "-D",
            "warnings"
        ]
    );
}

#[test]
fn cargo_success_fact_and_warnings_summary() {
    let reg = registry();
    let stdout = r#"{"reason":"compiler-message","message":{"level":"warning","message":"unused variable","code":{"code":"unused_variables"},"spans":[{"file_name":"src/lib.rs","line_start":2,"column_start":9,"is_primary":true}]}}
{"reason":"compiler-artifact","package_id":"demo 0.1.0 (path+file:///tmp/demo)","manifest_path":"/tmp/demo/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"demo","src_path":"/tmp/demo/src/lib.rs","edition":"2021","doc":true,"doctest":true,"test":true},"profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},"features":[],"filenames":[],"executable":null,"fresh":false}
{"reason":"build-finished","success":true}
"#;
    let run = reg
        .parse(
            &["cargo".to_string(), "check".to_string()],
            &Exit::Code(0),
            stdout,
            "",
        )
        .unwrap();
    assert_eq!(run.parser.as_deref(), Some("cargo"));
    assert_eq!(run.summary.as_deref(), Some("check ok, 1 warning"));
    assert_eq!(run.diagnostics.len(), 1);
    assert_eq!(run.diagnostics[0].severity, Severity::Warn);
}

#[test]
fn clean_success_facts_for_json_parsers() {
    let reg = registry();
    let eslint = reg
        .parse(
            &["eslint".into(), "-f".into(), "json".into()],
            &Exit::Code(0),
            "[]",
            "",
        )
        .unwrap();
    assert_eq!(eslint.summary.as_deref(), Some("eslint ok"));
    let ruff = reg
        .parse(
            &["ruff".into(), "check".into(), ".".into()],
            &Exit::Code(0),
            "[]",
            "",
        )
        .unwrap();
    assert_eq!(ruff.summary.as_deref(), Some("ruff ok"));
    let tsc = reg.parse(&["tsc".into()], &Exit::Code(0), "", "").unwrap();
    assert_eq!(tsc.summary.as_deref(), Some("tsc ok"));
}

#[test]
fn go_test_package_success_summary() {
    let reg = registry();
    let out = "ok  \texample.com/pkg\t0.123s\n";
    let run = reg
        .parse(
            &["go".into(), "test".into(), "./...".into()],
            &Exit::Code(0),
            out,
            "",
        )
        .unwrap();
    assert_eq!(run.summary.as_deref(), Some("1 packages ok"));
}

#[test]
fn pytest_no_tests_summary() {
    let reg = registry();
    let out = "collected 0 items\n\n========================= no tests ran in 0.01s =========================\n";
    let run = reg
        .parse(&["pytest".into(), "-q".into()], &Exit::Code(5), out, "")
        .unwrap();
    assert_eq!(run.summary.as_deref(), Some("pytest: no tests ran"));
}

#[test]
fn jest_empty_results_summary() {
    let reg = registry();
    let jest = serde_json::json!({"numTotalTests": 0, "testResults": []}).to_string();
    let run = reg
        .parse(&["jest".into(), "--json".into()], &Exit::Code(0), &jest, "")
        .unwrap();
    assert_eq!(run.summary.as_deref(), Some("0 tests"));
}

#[test]
fn cargo_help_and_note_children_are_kept() {
    let reg = registry();
    let stdout = r#"{"reason":"compiler-message","message":{"level":"error","message":"cannot find value `foo`","code":{"code":"E0425"},"spans":[{"file_name":"src/main.rs","line_start":2,"column_start":5,"is_primary":true}],"children":[{"level":"help","message":"a local variable with a similar name exists: `fop`"},{"level":"note","message":"second note"},{"level":"help","message":"third hint is dropped"}]}}"#;
    let run = reg
        .parse(
            &["cargo".to_string(), "check".to_string()],
            &Exit::Code(101),
            stdout,
            "",
        )
        .unwrap();
    let msg = &run.diagnostics[0].message;
    assert!(msg.contains("; help: a local variable"), "{msg}");
    assert!(msg.contains("second note"), "{msg}");
    assert!(!msg.contains("third hint"), "{msg}");
}

#[test]
fn eslint_suggestion_descriptions_are_kept() {
    let reg = registry();
    let eslint = json!([{
        "filePath": "src/a.js",
        "messages": [{
            "line": 4, "column": 2, "severity": 2,
            "ruleId": "no-unused-vars", "message": "unused",
            "suggestions": [{"desc": "Remove the unused variable."}]
        }]
    }])
    .to_string();
    let run = reg
        .parse(
            &["eslint".to_string(), "-f".to_string(), "json".to_string()],
            &Exit::Code(1),
            &eslint,
            "",
        )
        .unwrap();
    assert!(
        run.diagnostics[0]
            .message
            .contains("Remove the unused variable."),
        "{}",
        run.diagnostics[0].message
    );
}
