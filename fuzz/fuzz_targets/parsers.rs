#![no_main]
use libfuzzer_sys::fuzz_target;
use wxec_core::parsers::ParserSet;
use wxec_core::types::Exit;
use wxec_parsers::ParserRegistry;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let reg = ParserRegistry::new();
    let exits = [Exit::Code(0), Exit::Code(1), Exit::Code(2), Exit::Code(101)];
    let argvs: Vec<Vec<String>> = vec![
        vec!["cargo".into(), "test".into()],
        vec!["cargo".into(), "check".into()],
        vec!["cargo".into(), "clippy".into()],
        vec!["pytest".into()],
        vec!["python".into(), "-m".into(), "pytest".into()],
        vec!["jest".into()],
        vec!["vitest".into()],
        vec!["tsc".into()],
        vec!["eslint".into(), ".".into()],
        vec!["ruff".into(), "check".into()],
        vec!["go".into(), "test".into(), "./...".into()],
        vec!["go".into(), "vet".into(), "./...".into()],
        vec!["unknowncmd".into()],
    ];
    for argv in &argvs {
        for e in &exits {
            let _ = reg.parse(argv, e, &text, &text);
            let _ = reg.parse(argv, e, &text, "");
            let _ = reg.parse(argv, e, "", &text);
        }
    }
    // inject args path
    let _ = reg.inject_args(&["cargo".into(), "test".into()]);
    let _ = reg.inject_args(&argvs[data.len() % argvs.len()]);
});
