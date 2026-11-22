use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, MutexGuard};

use serde_json::json;
use wxec_core::config::Config;
use wxec_core::dispatcher::Dispatcher;
use wxec_core::formatter::Rendered;
use wxec_parsers::ParserRegistry;

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0);

fn setup() -> (PathBuf, Dispatcher, MutexGuard<'static, ()>) {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("wxec-backlog-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let cache = root.with_extension("cache");
    let _ = std::fs::remove_dir_all(&cache);
    let config_home = root.with_extension("config");
    let _ = std::fs::remove_dir_all(&config_home);
    let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);
    let mut cfg = Config::default();
    cfg.session.default_tier = "execute".to_string();
    let registry = Arc::new(wxec_tools::registry());
    let parsers = Arc::new(ParserRegistry::new());
    let dispatcher = Dispatcher::new(cfg, &root, registry, parsers).unwrap();
    (root, dispatcher, guard)
}

fn call(d: &mut Dispatcher, tool: &str, args: serde_json::Value) -> Rendered {
    d.call(tool, &args, "s")
}

fn handle_ids(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"~[a-z0-9]+").unwrap();
    let mut out = Vec::new();
    for m in re.find_iter(text) {
        let id = m.as_str().to_string();
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

#[test]
fn lossless_expand_recovers_elided_read_lines() {
    let (root, mut d, _g) = setup();
    let lines: Vec<String> = (1..=600).map(|i| format!("line {i} value")).collect();
    let content = lines.join("\n") + "\n";
    std::fs::write(root.join("big.txt"), &content).unwrap();

    let r = call(&mut d, "read", json!({"path":"big.txt"}));
    assert!(r.ok, "{}", r.text);
    assert!(r.text.contains("...+"), "{}", r.text);
    let ids = handle_ids(&r.text);
    assert!(!ids.is_empty(), "{}", r.text);

    let mut recovered = r.text.clone();
    let mut expanded = std::collections::BTreeSet::new();
    let mut queue = ids;
    while let Some(id) = queue.pop() {
        if !expanded.insert(id.clone()) {
            continue;
        }
        let ex = call(
            &mut d,
            "expand",
            json!({"ref": id, "all": true, "depth": 2}),
        );
        assert!(ex.ok, "{} -> {}", id, ex.text);
        recovered.push('\n');
        recovered.push_str(&ex.text);
        for nid in handle_ids(&ex.text) {
            if !expanded.contains(&nid) {
                queue.push(nid);
            }
        }
    }
    for needle in ["line 1 value", "line 300 value", "line 600 value"] {
        assert!(
            recovered.contains(needle),
            "missing {needle} in\n{recovered}"
        );
    }
}

#[test]
fn edge_case_files_crlf_long_and_boundaries() {
    let (root, mut d, _g) = setup();
    std::fs::write(root.join("crlf.txt"), "a\r\nb\r\nc\r\n").unwrap();
    let r = call(&mut d, "read", json!({"path":"crlf.txt"}));
    assert!(
        r.ok && r.text.contains("1: a") && r.text.contains("2: b"),
        "{}",
        r.text
    );

    let long = "x".repeat(250_000) + "\nend\n";
    std::fs::write(root.join("long.txt"), &long).unwrap();
    let r = call(&mut d, "read", json!({"path":"long.txt", "lines":"2:2"}));
    assert!(r.ok && r.text.contains("2: end"), "{}", r.text);

    for (name, n) in [
        ("b8191.txt", 8191usize),
        ("b8192.txt", 8192),
        ("b8193.txt", 8193),
    ] {
        let mut bytes = vec![b'a'; n.saturating_sub(1)];
        bytes.extend_from_slice("\u{00E9}".as_bytes());
        std::fs::write(root.join(name), &bytes).unwrap();
        let r = call(&mut d, "read", json!({"path": name, "lines":"1:1"}));
        assert!(r.ok, "{name} {}", r.text);
        assert!(!r.text.contains("!binary"), "{name} {}", r.text);
    }
}

#[test]
fn repeated_reads_unchanged_and_force() {
    let (root, mut d, _g) = setup();
    std::fs::write(root.join("f.txt"), "one\n").unwrap();
    let r1 = call(&mut d, "read", json!({"path":"f.txt"}));
    assert!(r1.ok && r1.text.contains("1: one"), "{}", r1.text);
    let r2 = call(&mut d, "read", json!({"path":"f.txt"}));
    assert!(r2.text.starts_with("unchanged f.txt"), "{}", r2.text);
    let r3 = call(&mut d, "read", json!({"path":"f.txt","force":true}));
    assert!(r3.ok && r3.text.contains("1: one"), "{}", r3.text);
    std::fs::write(root.join("f.txt"), "two\n").unwrap();
    let r4 = call(&mut d, "read", json!({"path":"f.txt"}));
    assert!(
        r4.ok && (r4.text.contains("two") || r4.text.contains("diff")),
        "{}",
        r4.text
    );
}
