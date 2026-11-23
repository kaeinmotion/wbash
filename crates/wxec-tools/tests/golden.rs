use std::path::{Path, PathBuf};
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

fn golden(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[test]
fn golden_wire_outputs() {
    let root: PathBuf = std::env::temp_dir().join(format!("wxec-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    copy_fixture(&root);
    let bigtext: String = (1..=120).map(|i| format!("line {i}\n")).collect();
    std::fs::write(root.join("big.txt"), bigtext).unwrap();
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

    assert_eq!(
        d.call("tree", &json!({}), "g").text,
        golden("tree/basic.lean")
    );
    assert_eq!(
        d.call("search", &json!({"pattern": "parse_config"}), "g")
            .text,
        golden("search/basic.lean")
    );
    assert_eq!(
        d.call("read", &json!({"path": "nope.rs"}), "g").text,
        golden("errors/not_found.lean")
    );

    let read = d.call("read", &json!({"path": "src/config.rs"}), "g");
    assert_eq!(read.text, golden("read/basic.lean"));
    let read2 = d.call("read", &json!({"path": "src/config.rs"}), "g");
    assert_eq!(read2.text, golden("read/unchanged.lean"));

    let edit = d.call("edit", &json!({"path":"src/config.rs","find":"path.to_string()","replace":"path.trim().to_string()"}), "g");
    assert_eq!(edit.text, golden("edit/basic.lean"));

    let big = d.call("read", &json!({"path":"big.txt","budget":64}), "g");
    let exp = d.call(
        "expand",
        &json!({"ref": extract_handle(&big.text), "budget":4000}),
        "g",
    );
    assert_eq!(exp.text, golden("expand/basic.lean"));

    let batch = d.call("batch", &json!({"calls":[{"tool":"read","args":{"path":"src/main.rs"}},{"tool":"read","args":{"path":"nope.rs"}}]}), "g");
    assert_eq!(batch.text, golden("batch/basic.lean"));

    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-qm", "init"])
        .current_dir(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    assert_eq!(
        d.call("git", &json!({"op":"status"}), "g").text,
        golden("git/status.lean")
    );
    assert_eq!(
        normalize_git(&d.call("git", &json!({"op":"log","limit":1}), "g").text),
        golden("git/log.lean")
    );

    d.set_session_tier("g", wxec_core::types::Tier::Execute)
        .unwrap();
    let run = d.call("run", &json!({"argv":["sh","-c","echo hi"]}), "g");
    assert_eq!(normalize_ms(&run.text), golden("run/basic.lean"));
    let check = d.call("check", &json!({"argv":["sh","-c","echo hi"]}), "g");
    assert_eq!(normalize_ms(&check.text), golden("check/basic.lean"));
    let sh = d.call("sh", &json!({"command":"echo hi"}), "g");
    assert_eq!(sh.text, golden("sh/denied.lean"));

    std::fs::write(root.join("dup.txt"), "x\nx\n").unwrap();
    let amb = d.call(
        "edit",
        &json!({"path":"dup.txt","find":"x","replace":"y"}),
        "g",
    );
    assert_eq!(amb.text, golden("errors/ambiguous.lean"));
}

fn extract_handle(text: &str) -> String {
    let re = regex::Regex::new(r"~[a-z0-9]+").unwrap();
    re.find_iter(text)
        .last()
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}
fn normalize_ms(text: &str) -> String {
    // durations >=1s are shown; <1s omitted in headers. Keep any shown duration stable.
    let re = regex::Regex::new(r", \d+(\.\d+)?(ms|s|m)").unwrap();
    re.replace(text, ", <dur>").to_string()
}
fn normalize_git(text: &str) -> String {
    let re = regex::Regex::new(r"(?m)^[0-9a-f]{7,40} ").unwrap();
    re.replace_all(text, "<hash> ").to_string()
}

#[test]
#[ignore]
fn generate_goldens() {
    let root: PathBuf =
        std::env::temp_dir().join(format!("wxec-golden-gen-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    copy_fixture(&root);
    let bigtext: String = (1..=120).map(|i| format!("line {i}\n")).collect();
    std::fs::write(root.join("big.txt"), bigtext).unwrap();
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
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden");
    let w = |rel: &str, s: &str| {
        let p = base.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, s).unwrap();
    };

    w("tree/basic.lean", &d.call("tree", &json!({}), "g").text);
    w(
        "search/basic.lean",
        &d.call("search", &json!({"pattern":"parse_config"}), "g")
            .text,
    );
    w(
        "errors/not_found.lean",
        &d.call("read", &json!({"path":"nope.rs"}), "g").text,
    );
    let read = d.call("read", &json!({"path":"src/config.rs"}), "g");
    w("read/basic.lean", &read.text);
    let read2 = d.call("read", &json!({"path":"src/config.rs"}), "g");
    w("read/unchanged.lean", &read2.text);
    // edit
    let edit = d.call("edit", &json!({"path":"src/config.rs","find":"path.to_string()","replace":"path.trim().to_string()"}), "g");
    w("edit/basic.lean", &edit.text);
    // expand: elide by tiny budget then recover
    let big = d.call("read", &json!({"path":"big.txt","budget":64}), "g");
    let exp = d.call(
        "expand",
        &json!({"ref": extract_handle(&big.text), "budget":4000}),
        "g",
    );
    w("expand/basic.lean", &exp.text);
    // batch
    let batch = d.call("batch", &json!({"calls":[{"tool":"read","args":{"path":"src/main.rs"}},{"tool":"read","args":{"path":"nope.rs"}}]}), "g");
    w("batch/basic.lean", &batch.text);
    // git
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.email", "t@t"])
        .current_dir(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["config", "user.name", "t"])
        .current_dir(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-qm", "init"])
        .current_dir(&root)
        .env("HOME", &root)
        .output()
        .unwrap();
    let gitst = d.call("git", &json!({"op":"status"}), "g");
    w("git/status.lean", &gitst.text);
    let gitlog = d.call("git", &json!({"op":"log","limit":1}), "g");
    w("git/log.lean", &normalize_git(&gitlog.text));
    // run/check/sh (sh denied by default)
    d.set_session_tier("g", wxec_core::types::Tier::Execute)
        .unwrap();
    let run = d.call("run", &json!({"argv":["sh","-c","echo hi"]}), "g");
    w("run/basic.lean", &normalize_ms(&run.text));
    let check = d.call("check", &json!({"argv":["sh","-c","echo hi"]}), "g");
    w("check/basic.lean", &normalize_ms(&check.text));
    let sh = d.call("sh", &json!({"command":"echo hi"}), "g");
    w("sh/denied.lean", &sh.text);
    // error
    let amb = {
        std::fs::write(root.join("dup.txt"), "x\nx\n").unwrap();
        d.call(
            "edit",
            &json!({"path":"dup.txt","find":"x","replace":"y"}),
            "g",
        )
    };
    w("errors/ambiguous.lean", &amb.text);
}
