//! Acceptance regressions for the four P1 verification findings.
use super::*;
use crate::context::{estimate_tokens, payload::json_tokens};
use serde_json::json;

#[test]
fn push_budget_includes_all_references_metadata_and_prompt_framing() {
    let temp = tests::root();
    let root = temp.path();
    let mut proposal = tests::proposal(root, None);
    proposal.evidence.clear();
    for i in 0..20 {
        let path = format!("contract-{i:02}-{}.txt", "source-".repeat(11));
        let text = "contract proof";
        std::fs::write(root.join(&path), text).unwrap();
        proposal.evidence.push(memory::Evidence {
            kind: "file".into(),
            reference: path,
            sha256: Some(digest(text.as_bytes())),
            revision: None,
            summary: "Source proof".into(),
        });
    }
    let fact = memory::propose(root, &proposal).unwrap();
    tests::activate(root, &fact, None);
    std::fs::write(
        root.join("worktree.prd.json"),
        json!({"tasks":[{"id":"A","acceptance_criteria":["preserve contract"]}]}).to_string(),
    )
    .unwrap();
    for limit in [1200, 100, 800, 0] {
        let view = policy::inspect(root).unwrap();
        let mut policy = view.policy;
        policy.context_engine.push_memory_limit_tokens = limit;
        policy::update(root, &view.revision, Some(policy)).unwrap();
        for cached in [false, true] {
            let prepared =
                store::checkpoints::prepare(root, &regression_tests::request(root, root, "A"))
                    .unwrap();
            let bundle = context::list(root).unwrap().remove(0);
            assert_eq!(bundle.cache_hit, cached);
            let optional = super::context_prompt::render(&bundle.selected, &bundle.id).unwrap();
            assert!(prepared.prompt.ends_with(&optional));
            assert_eq!(
                bundle.optional_estimated_tokens,
                estimate_tokens(&optional) as u64
            );
            assert!(bundle.optional_estimated_tokens <= limit);
            assert!(prepared.prompt.contains("preserve contract"));
            if limit <= 100 {
                assert!(bundle.selected.is_empty());
            } else {
                assert_eq!(bundle.selected[0].source_refs.len(), 20);
            }
        }
    }
}

#[test]
fn code_queries_bound_the_full_envelope_and_preserve_complete_hit_provenance() {
    let temp = tests::root();
    let root = temp.path();
    std::fs::write(
        root.join("contract.rs"),
        format!("needle {}\n", "plain words ".repeat(20)),
    )
    .unwrap();
    for limit in [100, 200, 400] {
        let view = policy::inspect(root).unwrap();
        let mut policy = view.policy;
        policy.code_intelligence.output_tokens = limit;
        policy::update(root, &view.revision, Some(policy)).unwrap();
        let result = providers::query(
            root,
            &providers::Query {
                text: "needle".into(),
                kind: "file_search".into(),
                worktree: None,
            },
        )
        .unwrap();
        let actual = json_tokens(&result).unwrap() as u64;
        assert!(actual <= limit);
        let providers::Capability::Supported(result) = result else {
            panic!()
        };
        assert_eq!(result.estimated_tokens, actual);
        if limit == 100 {
            assert!(result.hits.is_empty());
            assert!(result
                .diagnostics
                .iter()
                .any(|s| s.contains("Output bounded")));
        } else {
            let hit = &result.hits[0];
            assert_eq!(hit.path, "contract.rs");
            assert_eq!(hit.file_sha256.len(), 64);
            assert!(hit.provenance.contains("literal_match"));
        }
    }
}

#[test]
#[cfg(unix)]
fn changed_runtime_behind_unchanged_launcher_blocks_native_configuration_until_reprobed() {
    let temp = tests::root();
    let root = temp.path();
    probe_tests::enable(root);
    let mut spec = probe_tests::spec(root, "toml");
    let version = root.join(".native-version");
    let banner = "fixture banner ".repeat(20);
    std::fs::write(&version, format!("{banner}future-cli 1.0.0\n")).unwrap();
    let wrapper = format!(
        "#!/bin/sh\ncase \"$1\" in --version) cat '{}';; --help) echo help;; *) exit 99;; esac\n",
        version.display()
    );
    std::fs::write(&spec.command, wrapper).unwrap();
    let hash = digest(&std::fs::read(&spec.command).unwrap());
    spec.compaction.as_mut().unwrap().version_pattern = "future-cli [12]\\.0\\.0".into();
    std::fs::create_dir_all(root.join(".macc/tools.d")).unwrap();
    std::fs::write(root.join(".macc/tools.d/future.tool.json"),json!({"api_version":"v1","id":"future-tool","display_name":"Future","fields":[],"performer":spec}).to_string()).unwrap();
    assert_eq!(
        compaction::probe_spec(root, "future-tool", &spec)
            .unwrap()
            .state,
        "version_help_observed"
    );
    let lease = probe_tests::grant(root, &spec);
    let file = root.join(".native/settings.toml");
    // A version matching the broad declaration still invalidates the exact prior observation.
    // The version change is past the display's 160-character bound: hash full output.
    std::fs::write(&version, format!("{banner}future-cli 2.0.0\n")).unwrap();
    assert_eq!(hash, digest(&std::fs::read(&spec.command).unwrap()));
    let listed = compaction::inspect(root).unwrap();
    assert_eq!(
        listed
            .iter()
            .find(|s| s.tool_id == "future-tool")
            .unwrap()
            .state,
        "stale"
    );
    let options = compaction::execution_options(root, &lease.key).unwrap();
    assert!(!options.applied);
    assert!(options.reason.contains("runtime version changed"));
    assert!(!file.exists());
    // Staleness is durable and cannot be silently approved by another invocation.
    assert!(
        !compaction::execution_options(root, &lease.key)
            .unwrap()
            .applied
    );
    compaction::probe_spec(root, "future-tool", &spec).unwrap();
    assert!(
        compaction::execution_options(root, &lease.key)
            .unwrap()
            .applied
    );
    assert!(file.exists());
    let mut newer = spec.clone();
    newer.compaction.as_mut().unwrap().schema = "updated-profile".into();
    std::fs::write(root.join(".macc/tools.d/future.tool.json"),json!({"api_version":"v1","id":"future-tool","display_name":"Future","fields":[],"performer":newer}).to_string()).unwrap();
    compaction::probe_spec(root, "future-tool", &newer).unwrap();
    assert!(
        !compaction::execution_options(root, &lease.key)
            .unwrap()
            .applied
    );
    assert_eq!(
        compaction::inspect(root)
            .unwrap()
            .into_iter()
            .find(|s| s.tool_id == "future-tool")
            .unwrap()
            .state,
        "version_help_observed"
    );
    store::release(root, &lease.key, true).unwrap();
}
