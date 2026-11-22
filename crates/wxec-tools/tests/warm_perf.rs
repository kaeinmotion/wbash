use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use wxec_core::config::Config;
use wxec_core::dispatcher::Dispatcher;
use wxec_parsers::ParserRegistry;

#[test]
#[ignore]
fn warm_read_and_search_perf() {
    let n: usize = std::env::var("WXEC_PERF_FILES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(50_000);
    let root = std::env::temp_dir().join(format!("wxec-warmperf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let cache = root.with_extension("cache");
    let config_home = root.with_extension("config");
    std::env::set_var("XDG_CACHE_HOME", &cache);
    std::env::set_var("XDG_CONFIG_HOME", &config_home);

    let per_dir = 250usize;
    let dirs = n.div_ceil(per_dir);
    for d in 0..dirs {
        let folder = root.join(format!("d{d:04}"));
        std::fs::create_dir_all(&folder).unwrap();
        for f in 0..per_dir {
            std::fs::write(folder.join(format!("f{f:03}.rs")), "// filler\n").unwrap();
        }
    }
    std::fs::write(
        root.join("target.rs"),
        (0..40)
            .map(|i| format!("line {i} needle"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();

    let mut d = Dispatcher::new(
        Config::default(),
        &root,
        Arc::new(wxec_tools::registry()),
        Arc::new(ParserRegistry::new()),
    )
    .unwrap();
    // warm index and read path
    let _ = d.call("search", &json!({"pattern":"needle"}), "s");
    let _ = d.call("read", &json!({"path":"target.rs"}), "s");

    let mut read_ms = Vec::new();
    for _ in 0..30 {
        let t = Instant::now();
        let r = d.call("read", &json!({"path":"target.rs"}), "s");
        read_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        assert!(r.ok, "{}", r.text);
    }
    let mut search_ms = Vec::new();
    for _ in 0..30 {
        let t = Instant::now();
        let r = d.call("search", &json!({"pattern":"needle"}), "s");
        search_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        assert!(r.ok, "{}", r.text);
    }
    read_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    search_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rp95 = read_ms[(read_ms.len() * 95 / 100).saturating_sub(1)];
    let sp95 = search_ms[(search_ms.len() * 95 / 100).saturating_sub(1)];
    let read_limit: f64 = std::env::var("WXEC_WARM_READ_P95_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5.0);
    let search_limit: f64 = std::env::var("WXEC_WARM_SEARCH_P95_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(25.0);
    eprintln!(
        "warm read p95={rp95:.2}ms search p95={sp95:.2}ms files~{} limits=<{read_limit}/{search_limit}ms",
        dirs * per_dir
    );
    assert!(rp95 < read_limit, "read p95 {rp95}");
    assert!(sp95 < search_limit, "search p95 {sp95}");
    let _ = std::fs::remove_dir_all(root);
}
