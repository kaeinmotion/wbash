use std::path::PathBuf;

use wxec_core::args::parse_line_range;
use wxec_core::formatter::{render, RenderOptions};
use wxec_core::handles::HandleStore;
use wxec_core::hash;
use wxec_core::policy;
use wxec_core::redact::Redactor;
use wxec_core::types::{Format, ToolResult};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wxec-prop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn i1_must_keep_preserved_under_tight_budget() {
    let red = Redactor::new();
    let dir = tmp("i1");
    let mut hs = HandleStore::open("s", &dir, 3600, 8 << 20, Redactor::new()).unwrap();
    let mut tr = ToolResult::new("t", "hdr".to_string());
    tr.must("!fatal kaboom");
    for i in 0..200 {
        tr.low(format!("noise {i}"));
    }
    let out = render(
        &tr,
        RenderOptions {
            budget: 20,
            format: Format::Lean,
            handles: &mut hs,
            redactor: &red,
            passthrough_tokens: None,
            min_gain_pct: None,
            verbosity: None,
        },
    )
    .unwrap();
    assert!(out.text.contains("!fatal kaboom"), "{}", out.text);

    let mut hs2 = HandleStore::open("s2", &dir.join("b"), 3600, 8 << 20, Redactor::new()).unwrap();
    let mut tr2 = ToolResult::new("t", "header".to_string());
    for _ in 0..80 {
        tr2.must("ERR critical failure details must be kept");
    }
    let out2 = render(
        &tr2,
        RenderOptions {
            budget: 20,
            format: Format::Lean,
            handles: &mut hs2,
            redactor: &red,
            passthrough_tokens: None,
            min_gain_pct: None,
            verbosity: None,
        },
    )
    .unwrap();
    assert!(
        out2.text.contains("ERR critical failure details"),
        "{}",
        out2.text
    );
    assert!(
        out2.text.contains("# budget_exceeded_must_keep"),
        "{}",
        out2.text
    );
}

#[test]
fn i2_lossless_elision_resolves() {
    let red = Redactor::new();
    let dir = tmp("i2");
    let mut hs = HandleStore::open("s", &dir, 3600, 8 << 20, Redactor::new()).unwrap();
    let mut tr = ToolResult::new("t", "h".to_string());
    for i in 0..120 {
        tr.low(format!("line-{i}-payload"));
    }
    let out = render(
        &tr,
        RenderOptions {
            budget: 30,
            format: Format::Lean,
            handles: &mut hs,
            redactor: &red,
            passthrough_tokens: None,
            min_gain_pct: None,
            verbosity: None,
        },
    )
    .unwrap();
    let re = regex::Regex::new(r"~[a-z0-9]+").unwrap();
    let id = re.find(&out.text).unwrap().as_str();
    let got = hs.get(id).unwrap();
    let s = String::from_utf8_lossy(&got.0);
    for i in 0..120 {
        if out.text.contains(&format!("line-{i}-payload")) {
            continue;
        }
        assert!(
            s.contains(&format!("line-{i}-payload")),
            "missing {i} in handle"
        );
    }
}

#[test]
fn i3_determinism() {
    let dir = tmp("i3");
    for _ in 0..20 {
        let red = Redactor::new();
        let mut hs =
            HandleStore::open("s", &dir.join("a"), 3600, 8 << 20, Redactor::new()).unwrap();
        let mut tr = ToolResult::new("t", "hdr".to_string());
        tr.must("keep");
        for i in 0..50 {
            tr.low(format!("v{i}"));
        }
        let a = render(
            &tr,
            RenderOptions {
                budget: 40,
                format: Format::Lean,
                handles: &mut hs,
                redactor: &red,
                passthrough_tokens: None,
                min_gain_pct: None,
                verbosity: None,
            },
        )
        .unwrap()
        .text;
        let mut hs2 =
            HandleStore::open("s2", &dir.join("b"), 3600, 8 << 20, Redactor::new()).unwrap();
        let b = render(
            &tr,
            RenderOptions {
                budget: 40,
                format: Format::Lean,
                handles: &mut hs2,
                redactor: &red,
                passthrough_tokens: None,
                min_gain_pct: None,
                verbosity: None,
            },
        )
        .unwrap()
        .text;
        assert_eq!(a, b);
    }
}

#[test]
fn i4_hash_deterministic() {
    assert_eq!(hash::short("0123456789abcdef0123"), "b3:0123456789abcdef");
    assert_eq!(hash::hash_str("abc"), hash::hash_str("abc"));
    assert_ne!(hash::hash_str("abc"), hash::hash_str("abd"));
}

#[test]
fn i5_path_jail_property_samples() {
    let root = std::env::temp_dir().join(format!("wxec-jail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a"), "x").unwrap();
    let c = root.canonicalize().unwrap();
    for p in [
        "../x",
        "../../etc/passwd",
        "/etc/passwd",
        "a/../../x",
        "..",
        "a\0b",
    ] {
        let r = policy::resolve(&c, p, &[], false);
        assert!(r.is_err(), "{p} => {:?}", r);
    }
    assert!(policy::resolve(&c, "a", &[], false).is_ok());
}

#[test]
fn i6_schema_budget_tokens_estimate() {
    // heuristic: representative 9-tool schema stays well under budget
    let schema = serde_json::json!({"tools": (0..9).map(|i| serde_json::json!({"name": format!("t{i}"), "description":"desc","inputSchema":{"type":"object","properties":{},"additionalProperties":false}})).collect::<Vec<_>>()});
    let s = serde_json::to_string(&schema).unwrap();
    assert!(
        wxec_core::estimator::estimate_text(&s) <= 1500,
        "{}",
        s.len()
    );
}

#[test]
fn i7_handle_integrity() {
    let dir = tmp("i7");
    let mut hs = HandleStore::open("s", &dir, 3600, 8 << 20, Redactor::new()).unwrap();
    let payload = (0..500)
        .map(|i| format!("row {i} alpha"))
        .collect::<Vec<_>>()
        .join("\n");
    let h = hs.put("raw", payload.as_bytes(), "text").unwrap();
    let got = hs.get(&h.id).unwrap();
    assert_eq!(String::from_utf8(got.0).unwrap(), payload);
}

#[test]
fn arg_line_range_bounds() {
    assert!(parse_line_range("1:2", 10).is_ok());
    assert!(parse_line_range("0:1", 10).is_err());
    assert!(parse_line_range("3:2", 10).is_err());
    assert!(parse_line_range("1:20", 10).is_err());
}
