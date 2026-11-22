use std::path::PathBuf;

use wxec_core::estimator;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = args
        .first()
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/scrubbed"));
    let write = args.iter().any(|a| a == "--write-expected");
    let bpe = tiktoken_rs::cl100k_base().expect("cl100k_base");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("corpus dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    let mut errors = Vec::new();
    let mut rows = Vec::new();
    for path in &paths {
        let text = std::fs::read_to_string(path).expect("utf8 corpus");
        let actual = bpe.encode_with_special_tokens(&text).len();
        let est = estimator::estimate_text(&text) as usize;
        let err = (est as f64 - actual as f64).abs() / actual.max(1) as f64;
        println!(
            "{:<22} actual={:<5} est={:<5} chars={:<5} bytes={:<5} lines={:<4} err={:>6.1}%",
            path.file_name().unwrap().to_string_lossy(),
            actual,
            est,
            text.chars().count(),
            text.len(),
            text.lines().count(),
            err * 100.0
        );
        errors.push(err);
        rows.push((
            path.file_name().unwrap().to_string_lossy().to_string(),
            actual,
            est,
        ));
    }
    let mape = errors.iter().sum::<f64>() / errors.len() as f64 * 100.0;
    println!("MAPE {mape:.2}% over {} files", errors.len());
    if write {
        let out =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/expected/estimator.json");
        let mut body = String::from("{\n  \"tokenizer\": \"cl100k_base\",\n  \"files\": {\n");
        for (i, (name, actual, est)) in rows.iter().enumerate() {
            let comma = if i + 1 == rows.len() { "" } else { "," };
            body.push_str(&format!(
                "    \"{name}\": {{\"tokens\": {actual}, \"estimate\": {est}}}{comma}\n"
            ));
        }
        body.push_str("  }\n}\n");
        std::fs::write(out, body).expect("write expected");
        println!("wrote corpus/expected/estimator.json");
    }
}
