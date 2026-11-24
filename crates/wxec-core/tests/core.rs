use std::path::PathBuf;

use wxec_core::args::parse_line_range;
use wxec_core::config::{self, Config};
use wxec_core::formatter::{render, RenderOptions};
use wxec_core::handles::HandleStore;
use wxec_core::hash;
use wxec_core::policy;
use wxec_core::redact::Redactor;
use wxec_core::types::{Format, ToolResult};

static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wxec-test-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn core_primitives() {
    let cache = temp_dir("cache");
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("XDG_CACHE_HOME", cache.join("cache-home"));
    std::env::set_var("XDG_CONFIG_HOME", cache.join("config-home"));

    assert_eq!(hash::short("0123456789abcdef0123"), "b3:0123456789abcdef");

    let redactor = Redactor::new();
    let (red, n) = redactor.redact("aws AKIAABCDEFGHIJKLMNOP and token=supersecretvalue");
    assert!(n >= 1, "expected redaction: {red}");
    assert!(!red.contains("AKIAABCDEFGHIJKLMNOP"));

    let (ai_red, ai_n) = redactor.redact("keys: sk-proj-1234567890123456789012 and sk-ant-api03-12345678901234567890 and AIzaSyD1234567890123456789012345678901");
    assert_eq!(ai_n, 3, "expected 3 ai keys redacted: {ai_red}");
    assert!(!ai_red.contains("sk-proj-"));
    assert!(!ai_red.contains("sk-ant-"));
    assert!(!ai_red.contains("AIzaSyD"));

    assert!(Redactor::is_secret_key("AUTH_HEADER"));
    assert!(Redactor::is_secret_key("BEARER_TOKEN"));
    assert!(Redactor::is_secret_key("OPENAI_API_KEY"));

    let root = temp_dir("root");
    let croot = root.canonicalize().unwrap();
    std::fs::write(croot.join("a.txt"), "hello").unwrap();
    assert!(policy::resolve(&croot, "a.txt", &[], true).is_ok());
    let outside_err = policy::resolve(&croot, "../escape", &[], false).unwrap_err();
    assert_eq!(outside_err.code, "outside_workspace");
    assert!(outside_err.hint.as_deref().unwrap_or("").contains("WXEC_ALLOW_PATHS"));

    // Verify WXEC_ALLOW_PATHS dynamic allowlisting
    let outside_dir = temp_dir("outside");
    let coutside = outside_dir.canonicalize().unwrap();
    std::fs::write(coutside.join("allowed.txt"), "allowed").unwrap();
    let cfg = Config::default();
    assert!(!cfg.allowlist_paths().contains(&coutside));
    std::env::set_var("WXEC_ALLOW_PATHS", coutside.to_str().unwrap());
    assert!(cfg.allowlist_paths().contains(&coutside));
    assert!(policy::resolve(&croot, coutside.join("allowed.txt").to_str().unwrap(), &cfg.allowlist_paths(), true).is_ok());
    std::env::remove_var("WXEC_ALLOW_PATHS");

    assert!(parse_line_range("1:99", 3).is_err());
    assert_eq!(parse_line_range("10", 20).unwrap(), (10, 10));
    assert_eq!(parse_line_range("1-5", 20).unwrap(), (1, 5));
    assert_eq!(parse_line_range("1..5", 20).unwrap(), (1, 5));

    #[derive(serde::Deserialize)]
    struct TestRange {
        #[serde(
            default,
            deserialize_with = "wxec_core::args::deserialize_line_range_opt"
        )]
        lines: Option<String>,
    }
    let from_arr: TestRange = serde_json::from_str(r#"{"lines": [1, 30]}"#).unwrap();
    assert_eq!(from_arr.lines.as_deref(), Some("1-30"));
    let from_single_num: TestRange = serde_json::from_str(r#"{"lines": 10}"#).unwrap();
    assert_eq!(from_single_num.lines.as_deref(), Some("10-10"));
    let from_str: TestRange = serde_json::from_str(r#"{"lines": "5-15"}"#).unwrap();
    assert_eq!(from_str.lines.as_deref(), Some("5-15"));

    // Budget fitting: must-keep survives, elision is resolvable.
    let mut result = ToolResult::new("read", "header");
    result.must("ERROR must keep");
    for i in 0..80 {
        result.low(format!("line {i} {}", "x".repeat(60)));
    }
    let mut handles = HandleStore::open("test", &root, 3600, 10_000_000, Redactor::new()).unwrap();
    let rendered = render(
        &result,
        RenderOptions {
            budget: 64,
            format: Format::Lean,
            handles: &mut handles,
            redactor: &redactor,
            passthrough_tokens: None,
            min_gain_pct: None,
            verbosity: None,
        },
    )
    .unwrap();
    assert!(rendered.text.contains("ERROR must keep"));
    assert!(rendered.text.contains("...+"));
    let id = rendered
        .text
        .split('(')
        .find_map(|s| s.split(')').next().filter(|x| x.starts_with('~')))
        .expect("elision handle")
        .to_string();
    let (bytes, _) = handles.get(&id).unwrap();
    let payload = String::from_utf8(bytes).unwrap();
    assert!(payload.starts_with("line "), "payload={payload}");
    assert!(payload.contains("line 79 "));

    // Handle gc with zero TTL.
    let mut short = HandleStore::open("short", &root, 0, 10_000_000, Redactor::new()).unwrap();
    let h = short.put("run", b"payload", "text").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(short.gc() >= 1);
    let err = short.get(&h.id).unwrap_err();
    assert!(err.code == "handle_not_found" || err.code == "handle_expired");

    // Handle count capacity bounding.
    let mut bounded = HandleStore::open("bounded", &root, 3600, 10_000_000, Redactor::new())
        .unwrap()
        .with_max_handles(2);
    let h1 = bounded.put("h1", b"val1", "text").unwrap();
    let h2 = bounded.put("h2", b"val2", "text").unwrap();
    assert!(bounded.get(&h1.id).is_ok());
    assert!(bounded.get(&h2.id).is_ok());
    // Adding h3 exceeds max_handles=2; oldest access (h1 was accessed before h2) should evict
    let _h3 = bounded.put("h3", b"val3", "text").unwrap();
    assert!(bounded.get(&h1.id).is_err());
    assert!(bounded.get(&h2.id).is_ok());

    // Config trust roundtrip.
    let cfg = Config::default();
    cfg.validate().unwrap();
    config::trust_root(&root).unwrap();
    assert!(config::is_trusted(&root));
    let abs = root.canonicalize().unwrap();
    assert!(config::is_trusted(&abs));
}

#[test]
fn env_allowlist_supports_wildcards_and_denies_secrets() {
    let mut cfg = wxec_core::config::Config::default();
    cfg.exec.env_allow = vec!["PATH".into(), "CARGO_*".into(), "NODE_*".into()];
    let mut extra = std::collections::HashMap::new();
    extra.insert("CARGO_HOME".into(), "/tmp/cargo".into());
    extra.insert("NODE_OPTIONS".into(), "--x".into());
    let env = wxec_core::executor::build_env(&cfg, Some(&extra)).unwrap();
    assert_eq!(
        env.get("CARGO_HOME").map(String::as_str),
        Some("/tmp/cargo")
    );
    assert_eq!(env.get("NODE_OPTIONS").map(String::as_str), Some("--x"));

    let mut bad = std::collections::HashMap::new();
    bad.insert("OTHER".into(), "1".into());
    assert!(wxec_core::executor::build_env(&cfg, Some(&bad)).is_err());

    cfg.exec.env_allow.push("API_KEY".into());
    let mut secret = std::collections::HashMap::new();
    secret.insert("API_KEY".into(), "x".into());
    assert!(wxec_core::executor::build_env(&cfg, Some(&secret)).is_err());
}
