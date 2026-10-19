//! Actual adapter execution, including routed argv and native-session reuse.
use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn routed_identity_survives_create_resume_and_passive_capture() {
    let (temp, lib) = setup();
    let root = temp.path();
    let fake = root.join("fake-ai.sh");
    std::fs::write(&fake, r#"#!/bin/bash
if [[ "$1" == --version ]]; then echo 'future-cli 1.2'; exit 0; fi
[[ "$1" == --model && "$2" == routed-model ]] || exit 41
mode="$3"; sid="$4"
printf '%s\t%s\t%s\n' "$mode" "$sid" "$2" >> "$CALL_LOG"
ack="$(printf '%s\n' "${@: -1}" | sed -n '/^MACC_CHECKPOINT_ACK: /p')"
if [[ "$5" == --json ]]; then
  jq -nc --arg sid "$sid" --arg text "$ack" '{type:"result",session_id:$sid,result:($text+"\nMACC_TASK_RESULT: success_without_changes"),usage:{input:17,output:3}}'
else
  echo "session id: $sid"
  printf '%s\n' "$ack" 'MACC_TASK_RESULT: success_without_changes'
fi
"#).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = root.join(".macc/tool.json");
    let mut spec: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let performer = &mut spec["performer"];
    performer["command"] = json!(fake);
    performer["args"] = json!(["--model", "default-model", "new"]);
    for mode in ["create", "resume"] {
        performer["session"][mode]["command"] = json!(fake);
        performer["session"][mode]["args"] =
            json!(["--model", "default-model", mode, "{session_id}"]);
    }
    performer["usage"] = json!({"schema":"fake-result-v1","append_args":["--json"],"event_match":{"/type":"result"},"text":["/result"],"session_id":"/session_id","fields":{"input_uncached":"/usage/input","output":"/usage/output"}});
    performer["compaction"] = json!({"schema":"future-compaction-v1","version_pattern":"^future-cli [0-9]+\\.[0-9]+\\n?$","documentation":"https://example.test/future/version"});
    spec["model_tiers"] = json!({"heavy":{"model":"routed-model"}});
    std::fs::write(path, spec.to_string()).unwrap();
    let status = value(cli(root, &["status"], None));
    let mut policy = status["policy"].clone();
    policy["usage"]["capture_mode"] = json!(1);
    value(cli(
        root,
        &["policy", "--revision", status["revision"].as_str().unwrap()],
        Some(&policy),
    ));
    for _ in 0..2 {
        let out = runner(root, &lib, "A", "performer")
            .env("MACC_MODEL_TIER", "heavy")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let usage = value(cli(root, &["usage"], None));
    let records = usage["records"].as_array().unwrap();
    assert_eq!(records.len(), 2);
    for row in records {
        assert_eq!(row["model"], "routed-model");
        assert_eq!(row["provider_version"], "future-cli 1.2");
        assert_eq!(row["incremental"]["input_uncached"], 17);
        assert!(row["incremental"]["cost"].is_null());
    }
    let calls = std::fs::read_to_string(root.join("calls.tsv")).unwrap();
    let rows: Vec<_> = calls
        .lines()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .collect();
    assert_eq!(rows.len(), 2);
    assert_eq!((rows[0][0], rows[1][0]), ("create", "resume"));
    assert_eq!(rows[0][1], rows[1][1]);

    // Passive output preserves identity without inventing token measurements.
    let status = value(cli(root, &["status"], None));
    let mut policy = status["policy"].clone();
    policy["usage"]["capture_mode"] = json!(0);
    value(cli(
        root,
        &["policy", "--revision", status["revision"].as_str().unwrap()],
        Some(&policy),
    ));
    let out = runner(root, &lib, "A", "performer")
        .env("MACC_MODEL_TIER", "heavy")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let usage = value(cli(root, &["usage"], None));
    assert_eq!(usage["records"][0]["model"], "routed-model");
    assert_eq!(usage["records"][0]["provider_version"], "future-cli 1.2");
    assert!(usage["records"][0]["incremental"]["input_uncached"].is_null());
    assert_eq!(usage["totals"][0]["counters"]["input_uncached"], 34);
    let status = value(cli(root, &["status"], None));
    assert_eq!(status["sessions"].as_array().unwrap().len(), 1);
    assert!(status["sessions"][0]["owner_pid"].is_null());
}

#[test]
fn usage_transport_preserves_an_empty_argument_vector() {
    let output = Command::new("bash")
        .args([
            "-c",
            r#"
set -eu
source "$1"
session_backend=sqlite
sqlite_lease_key='{"session_id":"test"}'
checkpoint_phase=dev
session_service() { cat; }
prepare_usage future-tool
printf '%s' "$usage_plan"
"#,
            "--",
        ])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../adapters/shared/usage.sh"))
        .output()
        .unwrap();
    let plan = value(output);
    assert_eq!(plan["invocation"]["args"], json!([]));
}
