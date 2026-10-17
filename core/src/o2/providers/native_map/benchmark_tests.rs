//! Fixed mixed-language service/UI corpus: accuracy and work, not billed savings.
use super::*;
use std::time::Instant;

#[derive(Deserialize)]
struct Expected {
    path: String,
    selector: String,
    line: u64,
    signature: String,
}

#[test]
fn representative_multilanguage_accuracy_and_resource_comparison() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/o2-native-map");
    let t = crate::o2::tests::root();
    let root = t.path();
    for path in ["service.rs", "service.py", "service.ts", "view.tsx"] {
        std::fs::copy(fixture.join(path), root.join(path)).unwrap();
        crate::git::run_git_output_mapped(root, &["add", path], "Track benchmark corpus").unwrap();
    }
    let view = crate::o2::policy::inspect(root).unwrap();
    let mut p = view.policy;
    p.code_intelligence.native_map_enabled = true;
    crate::o2::policy::update(root, &view.revision, Some(p.clone())).unwrap();
    let started = Instant::now();
    let full = build(root, &p.code_intelligence, None).unwrap();
    let full_us = started.elapsed().as_micros();
    let started = Instant::now();
    let cached = build(root, &p.code_intelligence, Some(&full)).unwrap();
    let cached_us = started.elapsed().as_micros();
    assert_eq!(
        serde_json::to_value(&full.entries).unwrap(),
        serde_json::to_value(&cached.entries).unwrap()
    );
    assert_eq!(cached.stats.parsed_files, 0);
    assert_eq!(cached.stats.reused_files, full.stats.parsed_files);
    let expected: Vec<Expected> =
        serde_json::from_slice(&std::fs::read(fixture.join("gold.json")).unwrap()).unwrap();
    let mut native_hits = 0;
    let mut native_correct = 0;
    let mut literal_hits = 0;
    let mut literal_correct = 0;
    let mut native_us = 0;
    let mut literal_us = 0;
    for gold in &expected {
        for native in [false, true] {
            let query = Query {
                text: if native {
                    gold.selector.clone()
                } else {
                    gold.selector.rsplit('/').next().unwrap().into()
                },
                kind: if native {
                    "symbol_signature"
                } else {
                    "file_search"
                }
                .into(),
                path: Some(gold.path.clone()),
                worktree: None,
            };
            let start = Instant::now();
            let Capability::Supported(answer) = providers::query(root, &query).unwrap() else {
                panic!("Unsupported benchmark query")
            };
            let elapsed = start.elapsed().as_micros();
            let correct = answer
                .hits
                .iter()
                .filter(|h| h.path == gold.path && h.line == gold.line)
                .count();
            if native {
                assert_eq!(answer.hits.len(), 1, "{}", gold.selector);
                assert!(
                    answer.hits[0].text.ends_with(&gold.signature),
                    "{}: {:?}",
                    gold.selector,
                    answer.hits
                );
                native_hits += answer.hits.len();
                native_correct += correct;
                native_us += elapsed;
            } else {
                literal_hits += answer.hits.len();
                literal_correct += correct;
                literal_us += elapsed;
            }
        }
    }
    assert_eq!(native_correct, expected.len());
    assert_eq!(literal_correct, expected.len());
    assert!(
        literal_hits > native_hits,
        "literal false positives should be represented in the corpus"
    );
    let report = serde_json::json!({
        "recipe":RECIPE,"corpus":"fixed-service-ui-v1","queries":expected.len(),"languages":["rust","python","typescript","tsx"],
        "native":{"precision":native_correct as f64/native_hits as f64,"recall":native_correct as f64/expected.len() as f64,"query_us":native_us},
        "literal_baseline":{"precision":literal_correct as f64/literal_hits as f64,"recall":literal_correct as f64/expected.len() as f64,"query_us":literal_us},
        "full":{"build_us":full_us,"parsed_files":full.stats.parsed_files,"corpus_bytes":full.stats.corpus_bytes,"artifact_bytes":serde_json::to_vec(&full).unwrap().len()},
        "incremental":{"build_us":cached_us,"parsed_files":cached.stats.parsed_files,"reused_files":cached.stats.reused_files},
        "notice":"Local fixed corpus. Literal baseline is not semantic navigation. No external-provider, process RSS, production-task or billing claim. Timing is measured, not a portable speed threshold."
    });
    tracing::info!(report = %report, "Native map comparative evidence");
    if let Some(path) = std::env::var_os("MACC_O2_BENCHMARK_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
}
