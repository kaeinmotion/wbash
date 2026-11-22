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
    let root = std::env::temp_dir().join(format!("wxec-tools-{}", std::process::id()));
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

    let cache = root.with_extension("cache");
    let _ = std::fs::remove_dir_all(&cache);
    let config_home = root.with_extension("config");
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
    assert!(after_fuzzy.text.contains("Err(\"none\".into())"), "{}", after_fuzzy.text);

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
