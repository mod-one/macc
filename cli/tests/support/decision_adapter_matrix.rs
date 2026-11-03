//! Real shipped adapter wrappers and ToolSpecs; only provider executables are
//! replaced. No credential, network request or paid model invocation occurs.
use super::*;
use std::os::unix::fs::PermissionsExt;
#[test]
fn all_five_shipped_adapters_deliver_exact_decisions_on_initial_and_reused_invocations() {
    use macc_core::coordinator::decisions as dm;
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for tool in ["claude", "codex", "gemini", "agy", "vibe"] {
        let (temp, _) = setup();
        let root = temp.path();
        let paths = macc_core::ProjectPaths::from_root(root);
        let mut cfg = macc_core::load_canonical_config(&paths.config_path).unwrap();
        let p = dm::Policy {
            enabled: true,
            ..Default::default()
        };
        let c = cfg
            .automation
            .coordinator
            .get_or_insert_with(Default::default);
        c.prd_file = Some("worktree.prd.json".into());
        c.reference_branch = Some("main".into());
        c.decisions = Some(p.clone());
        macc_core::save_canonical_config(&paths, &cfg).unwrap();
        let (mut store, plan) = dm::active_plan(root).unwrap();
        let d=store.feedback(&plan,"A","Ne pas supprimer de fichiers. This exact sentence must reach every actual adapter.","Human",false,&p).unwrap();
        let fake = root.join("fake-ai.sh");
        std::fs::write(&fake,r#"#!/usr/bin/python3.12
import sys,os,json,re,uuid
args=sys.argv[1:]
if args==['--version']:
 print('fixture-provider 1.0.0');sys.exit(0)
text=args[-1]
assert 'This exact sentence must reach every actual adapter.' in text
assert 'Ne pas supprimer de fichiers.' in text
assert not any('{model}' in a or '{effort}' in a for a in args)
with open('.macc/provider-calls.jsonl','a') as f:f.write(json.dumps(args)+'\n')
lines=[s for s in text.splitlines() if s.startswith(('MACC_CHECKPOINT_ACK:','MACC_DECISION_ACK:'))]
lines.append('MACC_TASK_RESULT: success_without_changes')
lines.append('MACC_DECISION_COMPLIANCE: '+json.dumps({os.environ['FIXTURE_DECISION']:{'status':'pass','evidence':'Fixture exercised exact delivered instructions; no external effect'}}))
text='\n'.join(lines)
tool=os.environ['FIXTURE_TOOL'];sid=str(uuid.uuid4())
if '--session-id' in args:sid=args[args.index('--session-id')+1]
elif '-r' in args:sid=args[args.index('-r')+1]
elif '--resume' in args:sid=args[args.index('--resume')+1]
elif tool=='codex' and 'resume' in args:sid=args[args.index('resume')+1]
if tool=='claude':
 model=args[args.index('--model')+1] if '--model' in args else 'sonnet'
 print(json.dumps({'type':'assistant','message':{'content':[{'type':'text','text':text}]}}))
 print(json.dumps({'type':'result','session_id':sid,'result':text,'modelUsage':{model:{'inputTokens':17,'outputTokens':3,'cacheReadInputTokens':0,'cacheCreationInputTokens':0}},'total_cost_usd':0.01}))
elif tool=='codex' and '--json' in args:
 print(json.dumps({'type':'thread.started','thread_id':sid}))
 print(json.dumps({'type':'item.completed','item':{'type':'agent_message','text':text}}))
 print(json.dumps({'type':'turn.completed','thread_id':sid,'usage':{'input_tokens':17,'output_tokens':3,'cached_input_tokens':0}}))
elif tool=='gemini' and '--output-format' in args and args[args.index('--output-format')+1]=='stream-json':
 print(json.dumps({'type':'message','role':'assistant','content':text}))
 print(json.dumps({'type':'result','session_id':sid,'stats':{'input':17,'cached':0,'output_tokens':3}}))
else:
 print('session id: '+sid);print(text)
"#).unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut spec: Value = serde_yaml::from_str(
            &std::fs::read_to_string(source.join(format!("registry/tools.d/{tool}.tool.yaml")))
                .unwrap(),
        )
        .unwrap();
        spec["performer"]["command"] = json!(fake);
        for pointer in [
            "/performer/session/create",
            "/performer/session/resume",
            "/performer/retry",
        ] {
            if let Some(command) = spec.pointer_mut(pointer).and_then(Value::as_object_mut) {
                command.insert("command".into(), json!(fake));
            }
        }
        std::fs::write(root.join(".macc/tool.json"), spec.to_string()).unwrap();
        for invocation in 0..2 {
            if invocation==1 {
                let status=value(cli(root,&["status"],None));let mut policy=status["policy"].clone();policy["usage"]["capture_mode"]=json!(1);
                value(cli(root,&["policy","--revision",status["revision"].as_str().unwrap()],Some(&policy)));
            }
            let attempt = format!("{tool}-{invocation}");
            let (context, file) = store
                .issue(
                    &store.plan(&plan.id).unwrap(),
                    "A",
                    &attempt,
                    "dev",
                    root,
                    &p,
                )
                .unwrap();
            // The performer/phase caller supplies the native rendered package;
            // each shipped wrapper verifies the effective prompt at invocation.
            std::fs::write(
                root.join("prompt.txt"),
                format!("Implement task\n{}", context.render()),
            )
            .unwrap();
            let out = Command::new("bash")
                .arg(source.join(format!("adapters/{tool}/{tool}.performer.sh")))
                .args(["--repo"])
                .arg(root)
                .arg("--worktree")
                .arg(root)
                .arg("--tool-json")
                .arg(root.join(".macc/tool.json"))
                .arg("--prompt-file")
                .arg(root.join("prompt.txt"))
                .args(["--task-id", "A"])
                .env("MACC_SESSION_SERVICE_BIN", env!("CARGO_BIN_EXE_macc"))
                .env("MACC_INTERNAL_INVOCATION", "1")
                .env("MACC_SESSION_ROLE", "performer")
                .env("MACC_MODEL_TIER", "standard")
                .env("MACC_MODEL_ROUTING_MODE", "auto")
                .env("MACC_DECISION_CLI", env!("CARGO_BIN_EXE_macc"))
                .env("MACC_DECISION_ROOT", root)
                .env("MACC_DECISION_PLAN", &plan.id)
                .env("MACC_DECISION_ATTEMPT", &attempt)
                .env("MACC_DECISION_PHASE", "dev")
                .env("MACC_DECISION_CONTEXT_FILE", file)
                .env("FIXTURE_TOOL", tool)
                .env("FIXTURE_DECISION", &d.id)
                .current_dir(root)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{tool} invocation {invocation}: {}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            store
                .complete(
                    &plan.id,
                    &attempt,
                    "dev",
                    &String::from_utf8_lossy(&out.stdout),
                    &p,
                )
                .unwrap_or_else(|e| {
                    panic!(
                        "{tool}: {e}; output: {}",
                        String::from_utf8_lossy(&out.stdout)
                    )
                });
            let issued = store.context_history(&plan.id, "A").unwrap();
            let receipt = issued
                .iter()
                .find(|r| r["context"]["attempt_id"] == context.attempt_id)
                .unwrap();
            assert_eq!(receipt["receipt"]["acknowledged"], true, "{tool}");
        }
        let calls = std::fs::read_to_string(root.join(".macc/provider-calls.jsonl")).unwrap();
        assert_eq!(calls.lines().count(), 2, "{tool}: no hidden replay");
        store.verify().unwrap();
    }
}
