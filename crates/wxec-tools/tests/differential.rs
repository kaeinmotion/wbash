use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use serde_json::json;
use wxec_core::config::Config;
use wxec_core::dispatcher::Dispatcher;
use wxec_parsers::ParserRegistry;

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn copy_fixture(dst: &Path) {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/repos/basic");
    for rel in [
        "Cargo.toml",
        "src/config.rs",
        "src/main.rs",
        "tests/basic.rs",
    ] {
        let to = dst.join(rel);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(src.join(rel), to).unwrap();
    }
}

#[test]
fn search_and_tree_match_reference_tools() {
    let root: PathBuf = std::env::temp_dir().join(format!("wxec-diff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    copy_fixture(&root);
    let cache = root.with_extension("cache");
    let config_home = root.with_extension("config");
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    let mut d = Dispatcher::new(
        Config::default(),
        &root,
        Arc::new(wxec_tools::registry()),
        Arc::new(ParserRegistry::new()),
    )
    .unwrap();

    let ours = d.call("search", &json!({"pattern": "parse_config"}), "d");
    let ours: BTreeSet<(String, u32)> = ours
        .text
        .lines()
        .filter_map(|l| {
            let (path, rest) = l.split_once(':')?;
            let line = rest.split(':').next()?.parse().ok()?;
            Some((path.to_string(), line))
        })
        .collect();

    let grep = Command::new("grep")
        .args(["-rna", "--exclude-dir=.git", "parse_config", "."])
        .current_dir(&root)
        .output()
        .expect("grep available");
    let theirs: BTreeSet<(String, u32)> = String::from_utf8_lossy(&grep.stdout)
        .lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("./")?;
            let (path, rest) = rest.split_once(':')?;
            let line = rest.split(':').next()?.parse().ok()?;
            Some((path.to_string(), line))
        })
        .collect();
    assert_eq!(ours, theirs);

    let tree = d.call("tree", &json!({}), "d");
    let mut ours: BTreeSet<String> = BTreeSet::new();
    let mut stack: Vec<String> = Vec::new();
    for line in tree.text.lines().skip(1) {
        if line.trim().is_empty() || line.contains("...+") {
            continue;
        }
        let depth = (line.len() - line.trim_start().len()) / 2;
        stack.truncate(depth);
        let name = line.trim().split(" (").next().unwrap();
        if name.ends_with('/') {
            stack.push(name.trim_end_matches('/').to_string());
        } else {
            let mut path = stack.clone();
            path.push(name.to_string());
            ours.insert(path.join("/"));
        }
    }
    let find = Command::new("find")
        .args([".", "-type", "f", "-not", "-path", "./.git/*"])
        .current_dir(&root)
        .output()
        .expect("find available");
    let theirs: BTreeSet<String> = String::from_utf8_lossy(&find.stdout)
        .lines()
        .filter_map(|l| l.strip_prefix("./").map(|s| s.trim().to_string()))
        .collect();
    assert_eq!(ours, theirs, "tree files");
}

#[test]
fn run_parse_miss_preserves_raw_tail_and_single_full_handle() {
    let root: PathBuf = std::env::temp_dir().join(format!("wxec-diff-rt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    copy_fixture(&root);
    let cache = root.with_extension("cache");
    let config_home = root.with_extension("config");
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    let mut d = Dispatcher::new(
        Config::default(),
        &root,
        Arc::new(wxec_tools::registry()),
        Arc::new(ParserRegistry::new()),
    )
    .unwrap();
    d.set_session_tier("d", wxec_core::types::Tier::Execute)
        .unwrap();

    let r = d.call(
        "run",
        &json!({"argv": ["sh", "-c", "echo 'error: kaboom'; echo '  --> src/lib.rs:1:2'; exit 2"]}),
        "d",
    );
    assert!(!r.ok, "{}", r.text);
    assert!(r.text.contains("# parse_miss"), "{}", r.text);
    assert!(r.text.contains("error: kaboom"), "{}", r.text);
    assert!(r.text.contains("--> src/lib.rs:1:2"), "{}", r.text);
    let full_count = r.text.matches("full: ~").count();
    assert_eq!(full_count, 1, "{}", r.text);
}

#[test]
fn run_parse_miss_preserves_all_error_markers_in_handle() {
    let root: PathBuf = std::env::temp_dir().join(format!("wxec-diff-rt2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    copy_fixture(&root);
    let cache = root.with_extension("cache");
    let config_home = root.with_extension("config");
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    let mut d = Dispatcher::new(
        Config::default(),
        &root,
        Arc::new(wxec_tools::registry()),
        Arc::new(ParserRegistry::new()),
    )
    .unwrap();
    d.set_session_tier("d", wxec_core::types::Tier::Execute)
        .unwrap();

    let cmd = "i=1; while [ $i -le 40 ]; do echo noise$i; i=$((i+1)); done; echo 'error: first'; echo '  --> a.rs:1:1'; echo 'error: second'; exit 7";
    let r = d.call("run", &json!({"argv": ["sh", "-c", cmd]}), "d");
    assert!(r.text.contains("error: first"), "{}", r.text);
    assert!(r.text.contains("--> a.rs:1:1"), "{}", r.text);
    assert!(r.text.contains("error: second"), "{}", r.text);
}

#[test]
fn run_env_allowlist_patterns_apply() {
    let root: PathBuf = std::env::temp_dir().join(format!("wxec-diff-env-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    copy_fixture(&root);
    let cache = root.with_extension("cache");
    let config_home = root.with_extension("config");
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    let mut d = Dispatcher::new(
        Config::default(),
        &root,
        Arc::new(wxec_tools::registry()),
        Arc::new(ParserRegistry::new()),
    )
    .unwrap();
    d.set_session_tier("d", wxec_core::types::Tier::Execute)
        .unwrap();
    let r = d.call(
        "run",
        &json!({"argv": ["sh", "-c", "echo $CARGO_TEST_ENV_X"], "env": {"CARGO_TEST_ENV_X": "ok"}}),
        "d",
    );
    assert!(r.text.contains("ok"), "{}", r.text);
}

#[test]
fn run_failure_never_drops_any_error_marker_lines() {
    let root: PathBuf = std::env::temp_dir().join(format!("wxec-diff-rt3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    copy_fixture(&root);
    let cache = root.with_extension("cache");
    let config_home = root.with_extension("config");
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    let mut d = Dispatcher::new(
        Config::default(),
        &root,
        Arc::new(wxec_tools::registry()),
        Arc::new(ParserRegistry::new()),
    )
    .unwrap();
    d.set_session_tier("d", wxec_core::types::Tier::Execute)
        .unwrap();

    let cmd = "i=1; while [ $i -le 30 ]; do echo noise$i; i=$((i+1)); done; \
      echo 'error: alpha'; echo '  --> a.rs:1:1'; \
      i=1; while [ $i -le 30 ]; do echo mid$i; i=$((i+1)); done; \
      echo 'error: beta'; echo '  --> b.rs:2:2'; \
      i=1; while [ $i -le 30 ]; do echo tail$i; i=$((i+1)); done; exit 9";
    let r = d.call("run", &json!({"argv": ["sh", "-c", cmd]}), "d");
    assert!(r.text.contains("error: alpha"), "{}", r.text);
    assert!(r.text.contains("--> a.rs:1:1"), "{}", r.text);
    assert!(r.text.contains("error: beta"), "{}", r.text);
    assert!(r.text.contains("--> b.rs:2:2"), "{}", r.text);
}

#[test]
fn run_env_honors_cargo_target_dir() {
    let root: PathBuf = std::env::temp_dir().join(format!("wxec-diff-ctd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    copy_fixture(&root);
    let cache = root.with_extension("cache");
    let config_home = root.with_extension("config");
    let target = root.join("custom-target");
    std::fs::create_dir_all(&target).unwrap();
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    let mut d = Dispatcher::new(
        Config::default(),
        &root,
        Arc::new(wxec_tools::registry()),
        Arc::new(ParserRegistry::new()),
    )
    .unwrap();
    d.set_session_tier("d", wxec_core::types::Tier::Execute)
        .unwrap();
    let r = d.call(
        "run",
        &json!({"argv": ["sh", "-c", "echo $CARGO_TARGET_DIR"], "env": {"CARGO_TARGET_DIR": target.to_string_lossy()}}),
        "d",
    );
    assert!(r.text.contains("custom-target"), "{}", r.text);
}
