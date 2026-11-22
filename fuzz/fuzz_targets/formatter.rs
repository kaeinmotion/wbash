#![no_main]
use libfuzzer_sys::fuzz_target;
use std::sync::OnceLock;
use wxec_core::formatter::{render, RenderOptions};
use wxec_core::handles::HandleStore;
use wxec_core::redact::Redactor;
use wxec_core::types::{Format, ToolResult};

fn tmpdir() -> &'static tempfile::TempDir {
    static D: OnceLock<tempfile::TempDir> = OnceLock::new();
    D.get_or_init(|| tempfile::tempdir().expect("tmp"))
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let redactor = Redactor::new();
    let dir = tmpdir().path().join(format!("f{}", data.len() % 97));
    let _ = std::fs::create_dir_all(&dir);
    let mut handles = match HandleStore::open("fuzz", &dir, 3600, 8 << 20, Redactor::new()) {
        Ok(h) => h,
        Err(_) => return,
    };
    for budget in [16u32, 64, 200, 800] {
        for fmt in [Format::Lean, Format::Json] {
            let mut result =
                ToolResult::new("fuzz", text.lines().next().unwrap_or("hdr").to_string());
            result.must(text.lines().next().unwrap_or("must"));
            let mut i = 0usize;
            for line in text.lines().skip(1).take(400) {
                if i % 3 == 0 {
                    result.high(line.to_string());
                } else {
                    result.low(line.to_string());
                }
                i += 1;
            }
            let _ = render(
                &result,
                RenderOptions {
                    budget,
                    format: fmt,
                    handles: &mut handles,
                    redactor: &redactor, passthrough_tokens: None, min_gain_pct: None, verbosity: None,
                },
            );
        }
    }
});
