#![no_main]
use libfuzzer_sys::fuzz_target;
use std::path::{Path, PathBuf};
use wxec_core::policy;

fuzz_target!(|data: &[u8]| {
    let raw = String::from_utf8_lossy(data);
    let p = Path::new(raw.as_ref());
    let _ = policy::normalize(p);
    if let Ok(cwd) = std::env::current_dir() {
        let _ = policy::canonical_root(&cwd);
    }
    if let Ok(root) = std::env::temp_dir().canonicalize() {
        let _ = policy::resolve(&root, &raw, &[], false);
        let _ = policy::resolve(&root, &raw, &[], true);
        // synthetic nested roots
        let nested = root.join("a/b/c");
        let _ = std::fs::create_dir_all(&nested);
        let _ = policy::resolve(&nested, &raw, &[PathBuf::from("/nonexistent-allow")], false);
    }
    // null-byte and odd path forms
    if raw.contains('\0') {
        let _ = policy::normalize(Path::new(&raw.replace('\0', "/\0/")));
    }
});
