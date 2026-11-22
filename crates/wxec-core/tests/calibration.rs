use std::path::Path;

use serde_json::Value;

use wxec_core::estimator;

#[test]
fn estimator_mape_under_ten_percent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let expected: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("expected/estimator.json")).unwrap(),
    )
    .unwrap();
    let files = expected["files"].as_object().expect("files map");
    assert!(files.len() >= 12);
    let mut errors = Vec::new();
    for (name, info) in files {
        let text = std::fs::read_to_string(root.join("scrubbed").join(name)).unwrap();
        let actual = info["tokens"].as_u64().expect("tokens") as f64;
        let est = estimator::estimate_text(&text) as f64;
        let err = (est - actual).abs() / actual;
        assert!(err <= 0.35, "{name}: est {est} actual {actual}");
        errors.push(err);
    }
    let mape = errors.iter().sum::<f64>() / errors.len() as f64;
    println!(
        "estimator MAPE {:.2}% over {} files",
        mape * 100.0,
        errors.len()
    );
    assert!(mape <= 0.10, "MAPE {:.2}%", mape * 100.0);
}
