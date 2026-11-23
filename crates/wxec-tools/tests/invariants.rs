use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use wxec_core::config::Config;
use wxec_core::dispatcher::Dispatcher;
use wxec_core::formatter::Rendered;
use wxec_core::types::Tier;
use wxec_parsers::ParserRegistry;

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn setup() -> (PathBuf, Dispatcher) {
    setup_named("default")
}

fn setup_named(name: &str) -> (PathBuf, Dispatcher) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("wxec-tools-{}-{}-{}", name, std::process::id(), id));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("tests")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"t\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/config.rs"),
        "//! Config.\npub fn parse_config(path: &str) -> Result<String, String> {\n    if path.is_empty() { return Err(\"empty\".into()); }\n    Ok(path.to_string())\n}\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/main.rs"),
        "mod config;\nfn main() { let _ = config::parse_config(\"x\"); }\n",
    )
    .unwrap();
    std::fs::write(
        root.join("tests/basic.rs"),
        "#[test]\nfn works() { assert_eq!(2+2, 4); }\n",
    )
    .unwrap();
    let big: String = (1..=80)
        .map(|i| format!("line {i} {}\n", "x".repeat(50)))
        .collect();
    std::fs::write(root.join("big.txt"), big).unwrap();

    let cache = root.with_extension(format!("cache-{}", id));
    let _ = std::fs::remove_dir_all(&cache);
    let config_home = root.with_extension(format!("config-{}", id));
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    let cfg = Config::default();
    let registry = Arc::new(wxec_tools::registry());
    let parsers = Arc::new(ParserRegistry::new());
    let dispatcher = Dispatcher::new(cfg, &root, registry, parsers).unwrap();
    (root, dispatcher)
}

fn call(d: &mut Dispatcher, tool: &str, args: Value) -> Rendered {
    d.call(tool, &args, "s")
}

fn handle_id(text: &str) -> String {
    let re = regex::Regex::new(r"~[a-z0-9]+").unwrap();
    re.find_iter(text)
        .last()
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

#[test]
fn tool_invariants() {
    let (root, mut d) = setup();

    // tree + determinism
    let t1 = call(&mut d, "tree", json!({}));
    let t2 = call(&mut d, "tree", json!({}));
    assert_eq!(t1.text, t2.text);
    assert!(t1.text.starts_with("./ (5 files"), "{}", t1.text);
    assert!(t1.text.contains("src/\n  config.rs"));

    // read + verified unchanged
    let r1 = call(&mut d, "read", json!({"path": "src/config.rs"}));
    assert!(r1.text.contains("1: //! Config."));
    let r2 = call(&mut d, "read", json!({"path": "src/config.rs"}));
    assert!(
        r2.text.starts_with("unchanged src/config.rs (b3:"),
        "{}",
        r2.text
    );
    assert!(r2.text.contains("expand ~"), "{}", r2.text);
    let forced = call(
        &mut d,
        "read",
        json!({"path": "src/config.rs", "force": true}),
    );
    assert!(forced.text.contains("1: //! Config."), "{}", forced.text);

    // external change -> diff with new-file line numbers
    std::fs::write(
        root.join("src/config.rs"),
        "//! Config.\npub fn parse_config(path: &str) -> Result<String, String> {\n    if path.is_empty() { return Err(\"empty\".into()); }\n    Ok(path.to_string())\n}\n// added\n",
    )
    .unwrap();
    let r3 = call(&mut d, "read", json!({"path": "src/config.rs"}));
    assert!(r3.text.contains("diff"), "{}", r3.text);
    assert!(r3.text.contains("+6: // added"), "{}", r3.text);

    // search
    let s = call(&mut d, "search", json!({"pattern": "parse_config"}));
    assert!(s.text.starts_with("2 files, 2 matches"), "{}", s.text);
    assert!(s.text.contains("src/config.rs:2:"));

    // literal edit + hash move
    let e = call(
        &mut d,
        "edit",
        json!({"path": "src/config.rs", "find": "path.to_string()", "replace": "path.trim().to_string()"}),
    );
    assert!(e.ok, "{}", e.text);
    assert!(
        e.text.starts_with("edited src/config.rs (+1 -1)"),
        "{}",
        e.text
    );
    let after = call(&mut d, "read", json!({"path": "src/config.rs"}));
    assert!(after.text.contains("path.trim().to_string()"));

    // whitespace-tolerant fuzzy anchor match
    let fuzzy = call(
        &mut d,
        "edit",
        json!({
            "path": "src/config.rs",
            // Anchor has different indentation and trailing whitespace
            "find": "pub fn parse_config(path: &str) -> Result<String, String> {   \n     if path.is_empty() { return Err(\"empty\".into()); }",
            "replace": "pub fn parse_config(path: &str) -> Result<String, String> {\n    if path.is_empty() { return Err(\"none\".into()); }"
        }),
    );
    assert!(fuzzy.ok, "{}", fuzzy.text);
    let after_fuzzy = call(&mut d, "read", json!({"path": "src/config.rs"}));
    assert!(
        after_fuzzy.text.contains("Err(\"none\".into())"),
        "{}",
        after_fuzzy.text
    );

    // ambiguous anchor
    std::fs::write(root.join("dup.txt"), "x\nx\n").unwrap();
    let amb = call(
        &mut d,
        "edit",
        json!({"path": "dup.txt", "find": "x", "replace": "y"}),
    );
    assert!(amb.is_error);
    assert!(
        amb.text
            .starts_with("!ambiguous_anchor 2 matches in dup.txt at 1:1, 2:1"),
        "{}",
        amb.text
    );
    assert!(amb.text.ends_with('\n'));

    // path jail
    let jail = call(&mut d, "read", json!({"path": "/etc/passwd"}));
    assert!(jail.is_error);
    assert!(jail.text.starts_with("!outside_workspace"), "{}", jail.text);

    // tier gate for run
    let denied = call(&mut d, "run", json!({"argv": ["true"]}));
    assert!(denied.text.starts_with("!policy_denied"), "{}", denied.text);
    d.set_session_tier("s", Tier::Execute).unwrap();
    let run = call(
        &mut d,
        "run",
        json!({"argv": ["sh", "-c", "echo out; echo 'error: boom' >&2; exit 3"]}),
    );
    assert_eq!(run.cli_code, 3);
    assert!(run.text.starts_with("exit 3"), "{}", run.text);
    assert!(run.text.contains("error: boom"), "{}", run.text);
    assert!(run.text.contains("full: ~"), "{}", run.text);
    assert_eq!(run.text.matches("full: ~").count(), 1, "{}", run.text);

    let miss = call(
        &mut d,
        "run",
        json!({"argv": ["sh", "-c", "echo 'error[E1]: bad'; echo '  --> x.rs:1:1'; exit 2"]}),
    );
    assert!(miss.text.contains("# parse_miss"), "{}", miss.text);
    assert!(miss.text.contains("error[E1]: bad"), "{}", miss.text);
    assert!(miss.text.contains("--> x.rs:1:1"), "{}", miss.text);
    assert_eq!(miss.text.matches("full: ~").count(), 1, "{}", miss.text);
    let sh = call(&mut d, "sh", json!({"command": "echo nope"}));
    assert!(sh.text.starts_with("!policy_denied"), "{}", sh.text);
    assert!(sh.text.contains("allow_shell=true"), "{}", sh.text);

    // I2: read elision resolves through expand with exact omitted lines
    let big = call(&mut d, "read", json!({"path": "big.txt", "budget": 64}));
    assert!(big.text.contains("...+"), "{}", big.text);
    let id = handle_id(&big.text);
    assert!(id.starts_with('~'), "{}", big.text);
    let exp = call(&mut d, "expand", json!({"ref": id, "budget": 4000}));
    assert_eq!(exp.cli_code, 0);
    assert!(exp.text.contains("line 80 "), "{}", exp.text);
    assert!(exp.text.contains("line 9 "), "{}", exp.text);

    // batch keeps sibling failures inline
    let batch = call(
        &mut d,
        "batch",
        json!({"calls": [
            {"tool": "read", "args": {"path": "src/main.rs"}},
            {"tool": "edit", "args": {"path": "src/main.rs", "find": "zzz", "replace": "q"}}
        ]}),
    );
    assert!(
        batch.text.starts_with("1 ok, 1 error, 2 total"),
        "{}",
        batch.text
    );
    assert!(batch.text.contains("#2 edit error"), "{}", batch.text);
    assert!(batch.text.contains("!anchor_not_found"), "{}", batch.text);

    // JSON wire mode parses and omits duplicate meta.ok
    let j = call(
        &mut d,
        "search",
        json!({"pattern": "config", "format": "json"}),
    );
    let v: Value = serde_json::from_str(&j.text).unwrap();
    assert_eq!(v["ok"], true);
    assert!(v["meta"].get("ok").is_none());

    // git read + gated write
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .env("HOME", &root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    let st = call(&mut d, "git", json!({"op": "status"}));
    assert!(st.text.starts_with("branch main,"), "{}", st.text);
    let denied = call(&mut d, "git", json!({"op": "commit", "message": "x"}));
    assert!(denied.text.starts_with("!policy_denied"), "{}", denied.text);
    d.set_session_tier("s", Tier::GitWrite).unwrap();
    git(&["add", "."]);
    let committed = call(&mut d, "git", json!({"op": "commit", "message": "initial"}));
    assert!(committed.ok, "{}", committed.text);
    assert!(
        committed.text.starts_with("committed "),
        "{}",
        committed.text
    );

    let _ = Path::new(&root);
}

#[test]
fn search_stats_flag() {
    let (_root, mut d) = setup();
    let s = call(
        &mut d,
        "search",
        json!({"pattern": "config", "stats": true}),
    );
    assert!(s.text.contains("# stats: scanned"), "{}", s.text);
}

#[test]
fn test_ergonomic_tool_features() {
    let (root, mut d) = setup_named("ergonomic");
    d.set_session_tier("s", Tier::WriteWorkspace).unwrap();

    // 1. tree with dirs_only
    let tree_dirs = call(&mut d, "tree", json!({"dirs_only": true}));
    assert!(tree_dirs.ok, "{}", tree_dirs.text);
    assert!(tree_dirs.text.contains("src/"), "{}", tree_dirs.text);
    assert!(
        !tree_dirs.text.contains("main.rs"),
        "dirs_only should not show leaf files: {}",
        tree_dirs.text
    );

    // 2. read with start_line and end_line
    let read_range = call(
        &mut d,
        "read",
        json!({"path": "src/config.rs", "start_line": 1, "end_line": 2}),
    );
    assert!(read_range.ok, "{}", read_range.text);
    assert!(
        read_range.text.contains("1: //! Config."),
        "{}",
        read_range.text
    );
    assert!(!read_range.text.contains("3:"), "{}", read_range.text);

    // 3. edit with content creating a file without explicit create: true
    let edit_auto_create = call(
        &mut d,
        "edit",
        json!({"path": "new_created.txt", "content": "hello auto create"}),
    );
    assert!(edit_auto_create.ok, "{}", edit_auto_create.text);
    assert!(root.join("new_created.txt").exists());

    // 4. batch with stop_on_error stops after first failing call and reports skipped count
    let batch_stop = call(
        &mut d,
        "batch",
        json!({
            "calls": [
                {"tool": "read", "args": {"path": "nope1.txt"}},
                {"tool": "read", "args": {"path": "src/main.rs"}},
                {"tool": "read", "args": {"path": "nope2.txt"}}
            ],
            "stop_on_error": true
        }),
    );
    assert!(!batch_stop.ok, "{}", batch_stop.text);
    assert!(
        batch_stop
            .text
            .starts_with("0 ok, 1 error, 2 skipped, 3 total"),
        "{}",
        batch_stop.text
    );
    assert!(
        batch_stop.text.contains("#1 read error"),
        "{}",
        batch_stop.text
    );
    assert!(!batch_stop.text.contains("#2 read"), "{}", batch_stop.text);

    // 5. git commit with stage: true auto-stages untracked files
    d.set_session_tier("s", Tier::GitWrite).unwrap();
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .env("HOME", &root)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.email", "t@t"]);
    git(&["config", "user.name", "t"]);
    let commit_auto_stage = call(
        &mut d,
        "git",
        json!({"op": "commit", "message": "auto stage commit", "stage": true}),
    );
    assert!(commit_auto_stage.ok, "{}", commit_auto_stage.text);
    assert!(
        commit_auto_stage.text.starts_with("committed "),
        "{}",
        commit_auto_stage.text
    );
    let st = call(&mut d, "git", json!({"op": "status"}));
    assert!(st.text.contains("0 changed"), "{}", st.text);

    // 6. edit auto-creates non-existent parent directories
    let nested_create = call(
        &mut d,
        "edit",
        json!({"path": "deeply/nested/sub/folder/file.txt", "content": "nested content"}),
    );
    assert!(nested_create.ok, "{}", nested_create.text);
    assert!(root.join("deeply/nested/sub/folder/file.txt").exists());

    // 7. run accepts string command for argv
    d.set_session_tier("s", Tier::Execute).unwrap();
    let run_str = call(&mut d, "run", json!({"argv": "echo string_argv_ok"}));
    assert!(run_str.ok, "{}", run_str.text);

    // 8. replace accepts target and replacement aliases
    let repl_alias = call(
        &mut d,
        "replace",
        json!({"path": "deeply/nested/sub/folder/file.txt", "target": "nested", "replacement": "updated"}),
    );
    assert!(repl_alias.ok, "{}", repl_alias.text);
    let verify_read = call(
        &mut d,
        "read",
        json!({"path": "deeply/nested/sub/folder/file.txt"}),
    );
    assert!(
        verify_read.text.contains("updated content"),
        "{}",
        verify_read.text
    );

    // 9. path and action aliases across tools
    let tree_dir_alias = call(&mut d, "tree", json!({"dir": "src"}));
    assert!(tree_dir_alias.ok, "{}", tree_dir_alias.text);

    let read_file_alias = call(&mut d, "read", json!({"file": "src/main.rs"}));
    assert!(read_file_alias.ok, "{}", read_file_alias.text);

    let read_files_alias = call(
        &mut d,
        "read",
        json!({"files": ["src/main.rs", "src/config.rs"]}),
    );
    assert!(read_files_alias.ok, "{}", read_files_alias.text);

    let edit_file_alias = call(
        &mut d,
        "edit",
        json!({"file": "src/main.rs", "find": "mod config;", "replace": "mod config; // ok"}),
    );
    assert!(edit_file_alias.ok, "{}", edit_file_alias.text);

    let repl_file_alias = call(
        &mut d,
        "replace",
        json!({"file": "src/main.rs", "find": "// ok", "replace": "// verified"}),
    );
    assert!(repl_file_alias.ok, "{}", repl_file_alias.text);

    let batch_steps_alias = call(
        &mut d,
        "batch",
        json!({
            "steps": [
                {"tool": "read", "args": {"file": "src/main.rs"}},
                {"tool": "tree", "args": {"dir": "src"}}
            ]
        }),
    );
    assert!(batch_steps_alias.ok, "{}", batch_steps_alias.text);

    let git_action_alias = call(&mut d, "git", json!({"action": "status"}));
    assert!(git_action_alias.ok, "{}", git_action_alias.text);

    // 10. check accepts dir and target aliases
    let check_cargo = call(&mut d, "check", json!({"dir": ".", "target": "all"}));
    assert!(check_cargo.ok, "{}", check_cargo.text);

    // 11. search with dir alias
    let search_dir = call(&mut d, "search", json!({"pattern": "config", "dir": "src"}));
    assert!(search_dir.ok, "{}", search_dir.text);

    // 12. crlf line ending normalization in edit
    let crlf_file = root.join("crlf.txt");
    std::fs::write(&crlf_file, "first line\r\nsecond line\r\nthird line\r\n").unwrap();
    let edit_crlf = call(
        &mut d,
        "edit",
        json!({
            "path": "crlf.txt",
            "find": "second line\nthird line",
            "replace": "updated second\nupdated third"
        }),
    );
    assert!(edit_crlf.ok, "{}", edit_crlf.text);
    let crlf_after = std::fs::read_to_string(&crlf_file).unwrap();
    assert!(
        crlf_after.contains("\r\n"),
        "must retain CRLF line endings: {crlf_after:?}"
    );
    assert!(crlf_after.contains("updated second\r\nupdated third"));

    // 13. expand with id alias
    let big = call(&mut d, "read", json!({"path": "big.txt", "budget": 64}));
    let id = handle_id(&big.text);
    assert!(id.starts_with('~'));
    let exp_id = call(&mut d, "expand", json!({"id": id, "budget": 4000}));
    assert!(exp_id.ok, "{}", exp_id.text);

    // 14. git with path and msg alias
    let git_diff_path = call(&mut d, "git", json!({"op": "diff", "path": "crlf.txt"}));
    assert!(git_diff_path.ok, "{}", git_diff_path.text);

    d.set_session_tier("s", Tier::GitWrite).unwrap();
    let git_commit_msg = call(
        &mut d,
        "git",
        json!({"op": "commit", "msg": "test commit with msg alias", "stage": true}),
    );
    assert!(git_commit_msg.ok, "{}", git_commit_msg.text);

    // 15. git tag listing and creation
    let git_tag_create = call(
        &mut d,
        "git",
        json!({"op": "tag", "ref": "v1.0.0", "message": "release 1.0"}),
    );
    assert!(git_tag_create.ok, "{}", git_tag_create.text);
    let git_tags_list = call(&mut d, "git", json!({"op": "tags"}));
    assert!(git_tags_list.ok, "{}", git_tags_list.text);
    assert!(git_tags_list.text.contains("v1.0.0"));

    // 16. git remote op
    let git_remote = call(&mut d, "git", json!({"op": "remote"}));
    assert!(git_remote.ok, "{}", git_remote.text);

    // 17. edit with content on existing file (auto overwrite when create is not specified)
    let edit_overwrite = call(
        &mut d,
        "edit",
        json!({"path": "crlf.txt", "content": "overwritten content"}),
    );
    assert!(edit_overwrite.ok, "{}", edit_overwrite.text);
    let verify_read = call(&mut d, "read", json!({"path": "crlf.txt"}));
    assert!(verify_read.text.contains("overwritten content"));

    // 18. batch with params/input aliases
    let batch_call = call(
        &mut d,
        "batch",
        json!({
            "calls": [
                {"tool": "tree", "params": {"depth": 1}},
                {"name": "read", "input": {"path": "crlf.txt"}}
            ]
        }),
    );
    assert!(batch_call.ok, "{}", batch_call.text);

    // 19. string-encoded numeric arguments in tree and search
    let tree_str_depth = call(&mut d, "tree", json!({"depth": "2"}));
    assert!(tree_str_depth.ok, "{}", tree_str_depth.text);

    let search_ci = call(
        &mut d,
        "search",
        json!({"pattern": "OVERWRITTEN", "case_insensitive": true}),
    );
    assert!(search_ci.ok, "{}", search_ci.text);
    assert!(search_ci.text.contains("crlf.txt"));

    // 20. replace with pattern and to aliases
    let replace_alias = call(
        &mut d,
        "replace",
        json!({"pattern": "overwritten", "to": "custom_replaced", "path": "crlf.txt"}),
    );
    assert!(replace_alias.ok, "{}", replace_alias.text);
    let verify_replace = call(&mut d, "read", json!({"path": "crlf.txt"}));
    assert!(verify_replace.text.contains("custom_replaced"));

    // 21. read and edit with embedded line ranges in path
    let read_embedded_colon = call(&mut d, "read", json!({"path": "crlf.txt:1-1"}));
    assert!(read_embedded_colon.ok, "{}", read_embedded_colon.text);
    assert!(read_embedded_colon.text.contains("1: custom_replaced"));

    let read_embedded_hash = call(&mut d, "read", json!({"path": "crlf.txt#L1-L1"}));
    assert!(read_embedded_hash.ok, "{}", read_embedded_hash.text);

    // 22. git branch creation and checkout
    let git_create_branch = call(
        &mut d,
        "git",
        json!({"op": "branch", "branch": "feature-test"}),
    );
    assert!(git_create_branch.ok, "{}", git_create_branch.text);
    let git_checkout = call(
        &mut d,
        "git",
        json!({"op": "checkout", "branch": "feature-test"}),
    );
    assert!(git_checkout.ok, "{}", git_checkout.text);

    // 23. rust range syntax and L-prefixed ranges in read, plus ./ prefix in globs
    let read_rust_range = call(
        &mut d,
        "read",
        json!({"path": "crlf.txt", "lines": "1..=1"}),
    );
    assert!(read_rust_range.ok, "{}", read_rust_range.text);
    assert!(read_rust_range.text.contains("1: custom_replaced"));

    let read_l_prefix = call(
        &mut d,
        "read",
        json!({"path": "crlf.txt", "lines": "L1-L1"}),
    );
    assert!(read_l_prefix.ok, "{}", read_l_prefix.text);

    let tree_dot_glob = call(&mut d, "tree", json!({"glob": "./*.txt"}));
    assert!(tree_dot_glob.ok, "{}", tree_dot_glob.text);
    assert!(tree_dot_glob.text.contains("crlf.txt"));

    // 24. tool aliases (grep, cat, ls) and read directory auto-delegation
    let cat_call = call(&mut d, "cat", json!({"path": "crlf.txt"}));
    assert!(cat_call.ok, "{}", cat_call.text);

    let grep_call = call(&mut d, "grep", json!({"pattern": "custom_replaced"}));
    assert!(grep_call.ok, "{}", grep_call.text);

    let ls_call = call(&mut d, "ls", json!({"path": "."}));
    assert!(ls_call.ok, "{}", ls_call.text);

    let read_dir = call(&mut d, "read", json!({"path": "."}));
    assert!(read_dir.ok, "{}", read_dir.text);

    // 25. replace with empty default replacement
    let replace_del = call(
        &mut d,
        "replace",
        json!({"find": "custom_replaced", "path": "crlf.txt"}),
    );
    assert!(replace_del.ok, "{}", replace_del.text);

    // 26. edit append and prepend
    let append_call = call(
        &mut d,
        "edit",
        json!({"path": "crlf.txt", "content": "appended_line", "append": true}),
    );
    assert!(append_call.ok, "{}", append_call.text);
    let verify_append = call(&mut d, "read", json!({"path": "crlf.txt"}));
    assert!(verify_append.text.contains("appended_line"));

    let prepend_call = call(
        &mut d,
        "edit",
        json!({"path": "crlf.txt", "content": "prepended_line", "prepend": true}),
    );
    assert!(prepend_call.ok, "{}", prepend_call.text);
    let verify_prepend = call(&mut d, "read", json!({"path": "crlf.txt"}));
    assert!(verify_prepend.text.contains("prepended_line"));

    // 27. search whole word, git log with path, and batch with null args
    let word_search = call(
        &mut d,
        "search",
        json!({"pattern": "appended_line", "word": true}),
    );
    assert!(word_search.ok, "{}", word_search.text);

    let log_path = call(&mut d, "git", json!({"op": "log", "path": "crlf.txt"}));
    assert!(log_path.ok, "{}", log_path.text);

    let batch_null = call(
        &mut d,
        "batch",
        json!({
            "calls": [
                {"tool": "tree", "args": null}
            ]
        }),
    );
    assert!(batch_null.ok, "{}", batch_null.text);

    // 28. edit with handle resolution (@~id) for content, find, and replace
    let read_big = call(&mut d, "read", json!({"path": "big.txt", "budget": 64}));
    let hid = handle_id(&read_big.text);
    assert!(hid.starts_with('~'));
    let edit_handle_content = call(
        &mut d,
        "edit",
        json!({"path": "handle_paste.txt", "content": format!("@{hid}")}),
    );
    assert!(edit_handle_content.ok, "{}", edit_handle_content.text);
    let verify_paste = call(&mut d, "read", json!({"path": "handle_paste.txt"}));
    assert!(verify_paste.ok, "{}", verify_paste.text);
    assert!(verify_paste.text.contains("line 11"));
    // 29. edit with occurrence (1, -1), near_line, and all disambiguation
    let dup_file = root.join("duplicate.txt");
    std::fs::write(&dup_file, "target_line\nmiddle\ntarget_line\nend\ntarget_line\n").unwrap();

    // 29a. occurrence 1
    let edit_occ1 = call(
        &mut d,
        "edit",
        json!({"path": "duplicate.txt", "find": "target_line", "replace": "first_target", "occurrence": 1}),
    );
    assert!(edit_occ1.ok, "{}", edit_occ1.text);
    let verify_occ1 = call(&mut d, "read", json!({"path": "duplicate.txt", "lines": "1:1"}));
    assert!(verify_occ1.text.contains("1: first_target"));

    // 29b. occurrence -1 (last)
    let edit_last = call(
        &mut d,
        "edit",
        json!({"path": "duplicate.txt", "find": "target_line", "replace": "last_target", "occurrence": -1}),
    );
    assert!(edit_last.ok, "{}", edit_last.text);
    let verify_last = call(&mut d, "read", json!({"path": "duplicate.txt", "lines": "5:5"}));
    assert!(verify_last.text.contains("5: last_target"));

    // 29c. all: true replaces all remaining matches
    let edit_all = call(
        &mut d,
        "edit",
        json!({"path": "duplicate.txt", "find": "target_line", "replace": "middle_target", "all": true}),
    );
    assert!(edit_all.ok, "{}", edit_all.text);
    let verify_all = call(&mut d, "read", json!({"path": "duplicate.txt", "lines": "3:3"}));
    assert!(verify_all.text.contains("3: middle_target"));

    // 30. batch with compact: true produces Net Digest rollup
    let batch_compact = call(
        &mut d,
        "batch",
        json!({
            "calls": [
                {"tool": "edit", "args": {"path": "duplicate.txt", "find": "first_target", "replace": "c1"}},
                {"tool": "edit", "args": {"path": "duplicate.txt", "find": "last_target", "replace": "c2"}}
            ],
            "compact": true
        }),
    );
    assert!(batch_compact.ok, "{}", batch_compact.text);
    assert!(
        batch_compact.text.contains("2 ok"),
        "{}",
        batch_compact.text
    );
    assert!(
        !batch_compact.text.contains("#1 edit ok"),
        "compact batch should suppress per-step headers: {}",
        batch_compact.text
    );
}
