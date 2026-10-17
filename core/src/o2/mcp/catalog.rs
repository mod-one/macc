//! A fixed capability allowlist, never a projection of privileged operator actions.
use super::*;
pub fn names(p: &policy::Policy) -> Vec<&'static str> {
    let mut names = vec![
        "memory.search",
        "memory.get",
        "memory.related_to_diff",
        "context.get_checkpoint",
    ];
    if p.memory.agent_proposals_allowed {
        names.extend(["memory.propose", "memory.challenge"]);
    }
    if p.code_intelligence.enabled {
        names.extend(["context.search_sources", "code.query", "code.path"]);
    }
    names
}
fn string(max: u64) -> Value {
    json!({"type":"string","maxLength":max})
}
fn integer(default: u64, max: u64) -> Value {
    json!({"type":"integer","minimum":0,"maximum":max,"default":default})
}
pub(super) fn list(p: &policy::Policy) -> Value {
    let tools=names(p).into_iter().map(|name|{
        let (description,properties,required)=match name {
            "memory.search"|"memory.related_to_diff"=>("Read approved, current, in-scope evidence. Refine queries or follow cursors; oversized records return references for memory.get.",json!({"query":string(512),"cursor":integer(0,500),"limit":{"type":"integer","minimum":1,"maximum":20,"default":5}}),vec![]),
            "memory.get"=>("Read one approved in-scope record as bounded JSON fragments. Join fragments only when checksum is unchanged.",json!({"id":string(80),"offset":integer(0,100000),"max_chars":{"type":"integer","minimum":1,"maximum":4096,"default":512}}),vec!["id"]),
            "memory.propose"=>("Submit task-scoped evidence for operator review. Always proposed; cannot change task state, authority or memory status.",json!({"request_id":string(64),"kind":{"type":"string","enum":["constraint","discovery","decision","gotcha","api_contract","failed_attempt","performance_note","security_note","migration_note","provider_note","merge_note"]},"title":string(160),"body":string(p.memory.max_body_chars),"confidence":{"type":"number","minimum":0,"maximum":1},"evidence_paths":{"type":"array","minItems":1,"maxItems":10,"items":string(512)},"tags":{"type":"array","maxItems":20,"items":string(80)}}),vec!["request_id","kind","title","body","confidence","evidence_paths"]),
            "memory.challenge"=>("Propose an in-scope challenge for review. Does not invalidate, conflict or suppress the existing fact.",json!({"request_id":string(64),"id":string(80),"reason":string(500)}),vec!["request_id","id","reason"]),
            "context.get_checkpoint"=>("Read the invocation's own verified checkpoint summary. Never replaces mandatory PRD restoration or proves delivery.",json!({}),vec![]),
            _=>("Read bounded code references from this session's worktree. No arbitrary files, worktree override, commands, installation or provider mutation. code.path may return unsupported.",json!({"text":string(256),"kind":{"type":"string","enum":["file_search","symbol_definition","references","dependency_path","architecture_summary","impact_analysis"]},"path":string(512)}),vec!["text"]),
        };
        let write=["memory.propose","memory.challenge"].contains(&name);
        json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":!write,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})
    }).collect::<Vec<_>>();
    json!({"tools":tools})
}
