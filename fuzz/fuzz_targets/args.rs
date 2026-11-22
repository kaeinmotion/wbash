#![no_main]
use libfuzzer_sys::fuzz_target;
use serde::Deserialize;
use serde_json::Value;
use wxec_core::args::{default_budget, parse_args, parse_line_range, strip_common};

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct Loose {
    #[serde(default)]
    _s: Option<String>,
    #[serde(default)]
    _n: Option<i64>,
}

fuzz_target!(|data: &[u8]| {
    let s = String::from_utf8_lossy(data);
    for len in [0usize, 1, 2, 10, 80, 10_000] {
        let _ = parse_line_range(&s, len);
    }
    for tool in ["read", "run", "batch", "tree", "unknown_tool"] {
        let _ = default_budget(tool);
    }
    if let Ok(v) = serde_json::from_slice::<Value>(data) {
        let _ = strip_common(v.clone(), "read");
        let _ = parse_args::<Loose>(v, "read", "hint");
    }
    // common CLI-like fragments
    for frag in [s.as_ref(), "1:1", ":10", "10:", "0:0", "-1:2", "a:b"] {
        let _ = parse_line_range(frag, 100);
    }
});
