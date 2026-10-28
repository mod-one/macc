//! Human-friendly terminal formatting for O2 and Session Store commands.
//! Preserves exact JSON transport when `--json`, `MACC_INTERNAL_INVOCATION=1`,
//! or non-terminal stdout is used.

use macc_core::{o2::actions::Action, MaccError, Result};
use serde::Serialize;
use serde_json::Value;
use std::io::IsTerminal;

/// Returns true if the output should be raw machine JSON rather than human-formatted text.
pub fn should_output_json(explicit_json: bool) -> bool {
    explicit_json
        || std::env::var("MACC_INTERNAL_INVOCATION").is_ok_and(|v| v == "1")
        || !std::io::stdout().is_terminal()
}

pub fn emit_o2(action: &Action, value: &Value, explicit_json: bool) -> Result<()> {
    if should_output_json(explicit_json) {
        print!("{}", macc_core::context::payload::json_line(value)?);
        return Ok(());
    }
    println!("{}", format_o2(action, value));
    Ok(())
}

pub fn emit_session_store(kind: &str, value: impl Serialize, explicit_json: bool) -> Result<()> {
    let json_val =
        serde_json::to_value(&value).map_err(|e| MaccError::Validation(e.to_string()))?;
    if should_output_json(explicit_json) {
        println!(
            "{}",
            serde_json::to_string(&json_val).map_err(|e| MaccError::Validation(e.to_string()))?
        );
        return Ok(());
    }
    println!("{}", format_session_store(kind, &json_val));
    Ok(())
}

fn on_off(v: &Value) -> &'static str {
    if v.as_bool().unwrap_or(false) {
        "ON"
    } else {
        "OFF"
    }
}

fn val_str(v: &Value) -> String {
    match v {
        Value::Null => "—".into(),
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        Value::String(s) => s.clone(),
        _ => v.to_string(),
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 && bytes % (1024 * 1024) == 0 {
        format!("{} MiB ({bytes} B)", bytes / (1024 * 1024))
    } else if bytes >= 1024 && bytes % 1024 == 0 {
        format!("{} KiB ({bytes} B)", bytes / 1024)
    } else {
        format!("{bytes} B")
    }
}

pub fn format_session_store(kind: &str, v: &Value) -> String {
    match kind {
        "status" | "policy" => format_session_store_status(v),
        "usage" => format_session_store_usage(v),
        "migrate" | "rollback" | "upgrade" | "usage_upgrade" => {
            format_migration_report("session-store", kind, v)
        }
        "backup" => format!("Backup created: {}", val_str(v)),
        "recover" => format!("Quarantined expired dead-owner leases: {}", val_str(v)),
        "close" => "Session closed.".into(),
        "events" => format_session_events(v),
        "checkpoint" => format_checkpoint_detail(v),
        "reconcile" => format_reconcile(v),
        _ => serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()),
    }
}

fn format_session_store_status(v: &Value) -> String {
    let mut lines = Vec::new();
    let backend = v["backend"].as_str().unwrap_or("unknown").to_uppercase();
    let root = v["project_root"].as_str().unwrap_or("—");
    let pid = v["project_id"].as_str().unwrap_or("—");
    let rev = v["revision"].as_str().unwrap_or("—");
    let source = v["policy_source"].as_str().unwrap_or("defaults");
    let ckpt_ready = v["checkpoints_ready"].as_bool().unwrap_or(false);

    lines.push("MACC Session Store".to_string());
    lines.push(format!("  Project:     {root}"));
    lines.push(format!("  Project ID:  {pid}"));
    lines.push(format!(
        "  Backend:     {backend} (Checkpoints schema: {})",
        if ckpt_ready { "ready" } else { "not ready" }
    ));
    lines.push(format!("  Source:      {source}"));
    lines.push(format!("  Revision:    {rev}"));
    lines.push(String::new());

    let p = &v["policy"];
    lines.push("Effective Policy:".to_string());
    lines.push(format!(
        "  Lease:       TTL = {}s, Heartbeat = {}s",
        p["lease"]["ttl_seconds"].as_u64().unwrap_or(1800),
        p["lease"]["heartbeat_seconds"].as_u64().unwrap_or(30)
    ));
    lines.push(format!(
        "  Lifecycle:   Max idle = {}h",
        p["lifecycle"]["max_idle_hours"].as_u64().unwrap_or(24)
    ));
    let start_b = p["checkpoints"]["max_content_bytes"]
        .as_u64()
        .unwrap_or(16384);
    let finish_b = p["checkpoints"]["max_finish_content_bytes"]
        .as_u64()
        .unwrap_or(65536);
    lines.push(format!(
        "  Checkpoints: Rotate after = {} uses, Start limit = {}, Finish limit = {}",
        p["checkpoints"]["rotate_after_uses"].as_u64().unwrap_or(20),
        format_bytes(start_b),
        format_bytes(finish_b)
    ));
    let cap_mode = p["usage"]["capture_mode"].as_u64().unwrap_or(0);
    let max_out = p["usage"]["max_output_bytes"].as_u64().unwrap_or(1048576);
    lines.push(format!(
        "  Usage:       Capture mode = {} ({}), Parse limit = {}",
        cap_mode,
        if cap_mode == 1 {
            "structured machine output"
        } else {
            "passive"
        },
        format_bytes(max_out)
    ));
    lines.push(String::new());

    let sessions = v["sessions"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    lines.push(format!("Sessions ({}):", sessions.len()));
    if sessions.is_empty() {
        lines.push("  (no recorded sessions)".to_string());
    } else {
        for s in sessions.iter().take(25) {
            lines.push(format!(
                "  - [{}] task={} role={} tool={} uses={} gen={} owner={} (id={})",
                s["status"].as_str().unwrap_or("?"),
                s["task_id"].as_str().unwrap_or("?"),
                s["role"].as_str().unwrap_or("?"),
                s["tool_id"].as_str().unwrap_or("?"),
                s["use_count"].as_i64().unwrap_or(0),
                s["generation"].as_i64().unwrap_or(0),
                s["owner_pid"]
                    .as_i64()
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "none".into()),
                s["id"].as_str().unwrap_or("?")
            ));
        }
        if sessions.len() > 25 {
            lines.push(format!(
                "  ... and {} more (use --json for full list)",
                sessions.len() - 25
            ));
        }
    }

    let checkpoints = v["checkpoints"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    lines.push(String::new());
    lines.push(format!("Checkpoints ({}):", checkpoints.len()));
    if checkpoints.is_empty() {
        lines.push("  (no recorded checkpoints)".to_string());
    } else {
        for c in checkpoints.iter().take(15) {
            lines.push(format!(
                "  - [{}] task={} phase={} role={} tool={} reason={} (id={})",
                c["validation_state"].as_str().unwrap_or("?"),
                c["task_id"].as_str().unwrap_or("?"),
                c["phase"].as_str().unwrap_or("?"),
                c["role"].as_str().unwrap_or("?"),
                c["tool_id"].as_str().unwrap_or("?"),
                c["reason"].as_str().unwrap_or("?"),
                c["id"].as_str().unwrap_or("?")
            ));
        }
        if checkpoints.len() > 15 {
            lines.push(format!(
                "  ... and {} more (use --json for full list)",
                checkpoints.len() - 15
            ));
        }
    }
    lines.push(String::new());
    lines.push("Tip: Use `--json` for raw machine output.".to_string());
    lines.join("\n")
}

fn format_session_store_usage(v: &Value) -> String {
    let mut lines = Vec::new();
    let ready = v["ready"].as_bool().unwrap_or(false);
    let totals = v["totals"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    let records = v["records"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);

    lines.push(format!(
        "MACC Invocation Usage (schema ready: {})",
        if ready { "yes" } else { "no" }
    ));
    lines.push(String::new());
    lines.push(format!("Task/Phase Totals ({}):", totals.len()));
    if totals.is_empty() {
        lines.push("  (no usage totals recorded)".to_string());
    } else {
        for t in totals {
            let c = &t["counters"];
            lines.push(format!(
                "  - task={} tool={} phase={} calls={} (measured={}) | in={} read={} write={} out={} reasoning={}",
                t["task_id"].as_str().unwrap_or("?"),
                t["tool_id"].as_str().unwrap_or("?"),
                t["phase"].as_str().unwrap_or("?"),
                t["invocations"].as_u64().unwrap_or(0),
                t["measured_invocations"].as_u64().unwrap_or(0),
                val_str(&c["input_uncached"]),
                val_str(&c["cached_read"]),
                val_str(&c["cached_write"]),
                val_str(&c["output"]),
                val_str(&c["reasoning"]),
            ));
        }
    }

    lines.push(String::new());
    lines.push(format!("Recent Invocations ({}):", records.len()));
    if records.is_empty() {
        lines.push("  (no invocation receipts)".to_string());
    } else {
        for r in records.iter().take(20) {
            lines.push(format!(
                "  - [{}] task={} phase={} role={} tool={} model={} quality={} elapsed={}ms",
                r["outcome"]
                    .as_str()
                    .or_else(|| r["status"].as_str())
                    .unwrap_or("?"),
                r["task_id"].as_str().unwrap_or("?"),
                r["phase"].as_str().unwrap_or("?"),
                r["role"].as_str().unwrap_or("?"),
                r["tool_id"].as_str().unwrap_or("?"),
                r["model"].as_str().unwrap_or("—"),
                r["token_quality"].as_str().unwrap_or("?"),
                val_str(&r["elapsed_ms"]),
            ));
        }
        if records.len() > 20 {
            lines.push(format!(
                "  ... and {} more (use --json for full list)",
                records.len() - 20
            ));
        }
    }
    lines.join("\n")
}

fn format_migration_report(domain: &str, cmd: &str, v: &Value) -> String {
    let applied = v["applied"].as_bool().unwrap_or(false);
    let direction = v["direction"].as_str().unwrap_or(cmd);
    let sha = v["source_sha256"].as_str().unwrap_or("—");
    let imported = v["imported"].as_u64().unwrap_or(0);
    let quarantined = v["quarantined"].as_u64().unwrap_or(0);
    let mut lines = vec![
        format!(
            "MACC {} ({})",
            direction,
            if applied { "APPLIED" } else { "PREVIEW" }
        ),
        format!("  Source SHA256: {sha}"),
    ];
    if imported > 0 || quarantined > 0 {
        lines.push(format!(
            "  Imported:      {imported} (quarantined: {quarantined})"
        ));
    }
    if let Some(backup) = v["backup"].as_str() {
        lines.push(format!("  Backup:        {backup}"));
    }
    if !applied {
        let sub = cmd.replace('_', "-");
        lines.push(String::new());
        lines.push("Next action to apply:".to_string());
        lines.push(format!(
            "  macc {domain} {sub} --apply --expected-sha256 {sha}"
        ));
    }
    lines.join("\n")
}

fn format_session_events(v: &Value) -> String {
    let Some(rows) = v.as_array() else {
        return serde_json::to_string_pretty(v).unwrap_or_default();
    };
    let mut lines = vec![format!("Session Events ({}):", rows.len())];
    for r in rows {
        lines.push(format!(
            "  - [{}] {} {}",
            val_str(&r["at"]),
            r["type"].as_str().unwrap_or("?"),
            r["payload"]
        ));
    }
    lines.join("\n")
}

fn format_checkpoint_detail(v: &Value) -> String {
    let mut lines = vec![
        format!("Checkpoint {}", v["id"].as_str().unwrap_or("?")),
        format!(
            "  Task:     {} (phase: {}, role: {}, tool: {})",
            v["task_id"].as_str().unwrap_or("?"),
            v["phase"].as_str().unwrap_or("?"),
            v["role"].as_str().unwrap_or("?"),
            v["tool_id"].as_str().unwrap_or("?")
        ),
        format!("  Worktree: {}", v["worktree"].as_str().unwrap_or("?")),
        format!(
            "  Branch:   {} @ {}",
            v["branch"].as_str().unwrap_or("?"),
            v["source_revision"].as_str().unwrap_or("?")
        ),
        format!("  Reason:   {}", v["reason"].as_str().unwrap_or("?")),
        format!("  Next:     {}", v["next_action"].as_str().unwrap_or("?")),
    ];
    if let Some(files) = v["changed_files"].as_array() {
        lines.push(format!("  Changed files ({}):", files.len()));
        for f in files.iter().take(20) {
            lines.push(format!("    - {}", val_str(f)));
        }
    }
    lines.join("\n")
}

fn format_reconcile(v: &Value) -> String {
    let applied = v["applied"].as_bool().unwrap_or(false);
    let d = &v["diagnostic"];
    let id = d["checkpoint_id"].as_str().unwrap_or("?");
    let mut lines = vec![
        format!(
            "Checkpoint reconciliation · {}",
            if applied { "APPLIED" } else { "PREVIEW" }
        ),
        format!("Diagnostic: {}", val_str(&d["code"])),
        format!("Task: {}", val_str(&d["task_id"])),
        format!("Previous worker: {}", val_str(&d["previous_worktree"])),
        format!("Resume worker:   {}", val_str(&d["execution_worktree"])),
        format!(
            "Contract review required: {}",
            val_str(&d["requires_contract_review"])
        ),
        format!("Changed task fields: {}", d["changed_task_fields"]),
    ];
    if applied {
        lines.push(format!(
            "Replacement checkpoint: {}",
            val_str(&v["checkpoint"])
        ));
        lines.push(
            "Criteria remain unvalidated. Unblock the task, then resume the coordinator.".into(),
        );
    } else {
        lines.push("Review the changed contract with --json. Apply the reviewed plan:".into());
        lines.push(format!(
            "  macc session-store reconcile {id} --expected-sha256 {}",
            val_str(&v["current_sha256"])
        ));
    }
    lines.join("\n")
}

pub fn format_o2(action: &Action, v: &Value) -> String {
    match action {
        Action::Status | Action::PolicyFieldApply { .. } | Action::PolicyApply { .. } => {
            if matches!(action, Action::PolicyFieldApply { .. }) {
                let mut out = String::from("O2 setting applied.\n\n");
                out.push_str(&format_o2_status(v, false));
                out
            } else {
                format_o2_status(v, false)
            }
        }
        Action::PolicyFieldGet { .. } => format_o2_field_get(v),
        Action::PolicyFieldPreview { .. } => format_o2_field_preview(v),
        Action::UpgradePlan | Action::UpgradeApply { .. } => {
            format_migration_report("o2", "upgrade", v)
        }
        Action::ProviderPlan { .. } => format_provider_plan(v),
        Action::ProviderApply { .. } => format_provider_apply(v),
        Action::Providers => format_providers(v),
        Action::AgentStatus => format_agent_status(v),
        Action::SemanticStatus { .. } => format_semantic_status(v),
        Action::CompactionList => format_compaction_list(v),
        Action::CompactionProbe { .. } => format_compaction_probe(v),
        Action::ContextList => format_context_list(v),
        Action::ContextGet { .. } => format_context_get(v),
        Action::MemoryPull { .. } | Action::MemorySearch { .. } | Action::MemoryExplain { .. } => {
            format_memory_search(v)
        }
        Action::MemoryConflicts => format_memory_conflicts(v),
        _ => serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string()),
    }
}

pub fn format_o2_policy_list(v: &Value) -> String {
    format_o2_status(v, true)
}

fn format_o2_status(v: &Value, show_all_fields: bool) -> String {
    let mut lines = Vec::new();
    let root = v["project_root"].as_str().unwrap_or("—");
    let sqlite = v["sqlite"].as_bool().unwrap_or(false);
    let ready = v["ready"].as_bool().unwrap_or(false);
    let source = v["source"].as_str().unwrap_or("defaults");
    let rev = v["revision"].as_str().unwrap_or("—");

    lines.push("MACC O2 Status".to_string());
    lines.push(format!("  Project:   {root}"));
    lines.push(format!(
        "  Storage:   {} (O2 schema: {})",
        if sqlite {
            "SQLite"
        } else {
            "JSON (migration required)"
        },
        if ready { "ready" } else { "upgrade required" }
    ));
    lines.push(format!("  Source:    {source}"));
    lines.push(format!("  Revision:  {rev}"));
    lines.push(String::new());

    let p = &v["policy"];
    let mem = &p["memory"];
    let ctx = &p["context_engine"];
    let cmp = &p["adaptive_compaction"];
    let cod = &p["code_intelligence"];

    lines.push("Modules & Key Settings:".to_string());
    lines.push(format!(
        "  [{}] Memory Ledger          (memory.enabled = {})",
        on_off(&mem["enabled"]),
        val_str(&mem["enabled"])
    ));
    lines.push(format!(
        "        Phase extraction:     {} (memory.phase_extraction_enabled)",
        val_str(&mem["phase_extraction_enabled"])
    ));
    lines.push(format!(
        "        Scoped stdio MCP:     {} (memory.mcp_enabled)",
        val_str(&mem["mcp_enabled"])
    ));
    lines.push(format!(
        "        Agent proposals:      {} (memory.agent_proposals_allowed)",
        val_str(&mem["agent_proposals_allowed"])
    ));
    lines.push(format!(
        "        Semantic search:      {} (memory.semantic_enabled, backend: {})",
        val_str(&mem["semantic_enabled"]),
        val_str(&mem["semantic_backend"])
    ));
    lines.push(format!(
        "        Limits:               search_limit = {}, max_body_chars = {}, min_confidence = {}",
        val_str(&mem["search_limit"]),
        val_str(&mem["max_body_chars"]),
        val_str(&mem["min_confidence"])
    ));

    lines.push(format!(
        "  [{}] Context Engine         (context_engine.enabled = {})",
        on_off(&ctx["enabled"]),
        val_str(&ctx["enabled"])
    ));
    lines.push(format!(
        "        Output reduction:     {} (max_output_bytes = {})",
        val_str(&ctx["output_reduction"]),
        val_str(&ctx["max_output_bytes"])
    ));
    lines.push(format!(
        "        Token budgets:        push_memory = {} tokens, pull = {} tokens",
        val_str(&ctx["push_memory_limit_tokens"]),
        val_str(&ctx["pull_limit_tokens"])
    ));
    lines.push(format!(
        "        Policy trials:        {} (context_engine.tuning_enabled)",
        val_str(&ctx["tuning_enabled"])
    ));

    lines.push(format!(
        "  [{}] Adaptive Compaction    (adaptive_compaction.enabled = {})",
        on_off(&cmp["enabled"]),
        val_str(&cmp["enabled"])
    ));
    lines.push(format!(
        "        Ratios (warn/act/grd): {} / {} / {} (min_turns = {}, max_compactions = {})",
        val_str(&cmp["warning_ratio"]),
        val_str(&cmp["action_ratio"]),
        val_str(&cmp["guard_ratio"]),
        val_str(&cmp["min_turns"]),
        val_str(&cmp["max_compactions_per_session"])
    ));
    lines.push(format!(
        "        Configure native auto: {}",
        val_str(&cmp["configure_native_auto"])
    ));

    lines.push(format!(
        "  [{}] Code Intelligence      (code_intelligence.enabled = {})",
        on_off(&cod["enabled"]),
        val_str(&cod["enabled"])
    ));
    lines.push(format!(
        "        Routing:              {} (native_map = {}, serena = {}, graphify = {})",
        val_str(&cod["routing"]),
        val_str(&cod["native_map_enabled"]),
        val_str(&cod["serena_enabled"]),
        val_str(&cod["graphify_enabled"])
    ));
    lines.push(format!(
        "        Query budgets:        query_limit = {}, output_tokens = {}",
        val_str(&cod["query_limit"]),
        val_str(&cod["output_tokens"])
    ));

    if let Some(fields) = v["fields"].as_array() {
        let mut overrides = Vec::new();
        for f in fields {
            let path = f["path"].as_str().unwrap_or("");
            let ptr = format!("/{}", path.replace('.', "/"));
            if let Some(cur) = p.pointer(&ptr) {
                if cur != &f["default"] {
                    overrides.push((path, cur, &f["default"], f["label"].as_str().unwrap_or("")));
                }
            }
        }
        lines.push(String::new());
        if overrides.is_empty() {
            lines.push("Non-default Settings: none (all fields at default values)".to_string());
        } else {
            lines.push(format!("Non-default Settings ({}):", overrides.len()));
            for (path, cur, def, label) in overrides {
                lines.push(format!(
                    "  * {path} = {} (default: {}) — {label}",
                    val_str(cur),
                    val_str(def)
                ));
            }
        }

        if show_all_fields {
            lines.push(String::new());
            lines.push(format!("All Configurable O2 Fields ({}):", fields.len()));
            for f in fields {
                let path = f["path"].as_str().unwrap_or("");
                let ptr = format!("/{}", path.replace('.', "/"));
                let cur = p.pointer(&ptr).unwrap_or(&Value::Null);
                let def = &f["default"];
                let marker = if cur != def { "*" } else { " " };
                let range = if f["kind"] == "bool" {
                    "bool".to_string()
                } else if let Some(choices) = f["choices"].as_array().filter(|c| !c.is_empty()) {
                    choices
                        .iter()
                        .filter_map(|c| c.as_str())
                        .collect::<Vec<_>>()
                        .join("|")
                } else if f["kind"] == "number" {
                    format!("{}..{}", val_str(&f["min"]), val_str(&f["max"]))
                } else {
                    f["kind"].as_str().unwrap_or("").to_string()
                };
                lines.push(format!(
                    "  {marker} {:<44} = {:<12} (default: {:<10} [{range}])",
                    path,
                    val_str(cur),
                    val_str(def)
                ));
            }
        }
    }

    if !sqlite {
        lines.push(String::new());
        lines
            .push("Next action: Run `macc session-store migrate` to enable SQLite storage.".into());
    } else if !ready {
        lines.push(String::new());
        lines.push("Next action: Run `macc o2 upgrade` to install the O2 schema.".into());
    } else if !show_all_fields {
        lines.push(String::new());
        lines.push(
            "Tip: Use `macc o2 policy list` for all fields, `macc o2 policy set <field> <value> --apply` to edit, or `--json` for raw JSON."
                .into(),
        );
    }
    lines.join("\n")
}

fn format_o2_field_get(v: &Value) -> String {
    let f = &v["field"];
    let path = f["path"].as_str().unwrap_or("?");
    let label = f["label"].as_str().unwrap_or("");
    let val = &v["value"];
    let def = &f["default"];
    let rev = v["revision"].as_str().unwrap_or("—");
    format!(
        "{path} = {} (default: {})\n  Label:      {label}\n  Type:       {}\n  Activation: {}\n  Revision:   {rev}",
        val_str(val),
        val_str(def),
        f["kind"].as_str().unwrap_or("?"),
        f["activation"].as_str().unwrap_or("?")
    )
}

fn format_o2_field_preview(v: &Value) -> String {
    if v["cancelled"].as_bool().unwrap_or(false) {
        return "O2 policy change cancelled (not applied).".into();
    }
    let path = v["path"].as_str().unwrap_or("?");
    let before = val_str(&v["before"]);
    let after = val_str(&v["after"]);
    let rev = v["revision"].as_str().unwrap_or("—");
    let act = v["activation"].as_str().unwrap_or("—");
    format!(
        "Preview O2 Policy Edit:\n  Field:      {path}\n  Change:     {before} -> {after}\n  Activation: {act}\n  Revision:   {rev}\n\nTo apply:\n  macc o2 policy set {path} {after} --apply\n  (or non-interactively: macc o2 policy set {path} {after} --apply --confirm --revision {rev})"
    )
}

fn format_provider_plan(v: &Value) -> String {
    let checksum = v["checksum"].as_str().unwrap_or("—");
    let res = &v["result"];
    let outcome = res["outcome"].as_str().unwrap_or("unknown");
    if outcome != "supported" {
        return format!(
            "Provider Plan ({outcome}):\n  Capability: {}\n  Reason:     {}",
            val_str(&res["data"]["capability"]),
            val_str(&res["data"]["reason"])
        );
    }
    let d = &res["data"];
    let id = d["id"].as_str().unwrap_or("—");
    let provider = d["provider_id"].as_str().unwrap_or("?");
    let version = d["provider_version"].as_str().unwrap_or("?");
    let action = d["action"].as_str().unwrap_or("?");
    let origin = d["package_origin"].as_str().unwrap_or("?");
    let network = d["network"].as_bool().unwrap_or(false);
    let rev = d["source_revision"].as_str().unwrap_or("—");

    let mut lines = vec![
        format!("MACC Provider Plan: {provider} ({action})"),
        format!("  Plan ID:         {id}"),
        format!("  Checksum:        {checksum}"),
        format!("  Provider:        {provider} v{version} ({origin})"),
        format!(
            "  Network access:  {}",
            if network { "yes" } else { "no (offline/local)" }
        ),
        format!("  Source revision: {rev}"),
    ];
    if let Some(paths) = d["affected_paths"].as_array() {
        if paths.is_empty() {
            lines.push("  Affected paths:  none".into());
        } else {
            lines.push(format!("  Affected paths ({}):", paths.len()));
            for p in paths {
                lines.push(format!("    - {}", val_str(p)));
            }
        }
    }
    if let Some(adapter) = d["details"]["adapter"].as_object() {
        lines.push("  Adapter details:".into());
        for (k, val) in adapter {
            lines.push(format!("    {k}: {}", val_str(val)));
        }
    }
    if let Some(diags) = d["diagnostics"].as_array().filter(|a| !a.is_empty()) {
        lines.push("  Diagnostics:".into());
        for diag in diags {
            lines.push(format!("    * {}", val_str(diag)));
        }
    }
    lines.push(String::new());
    lines.push("Next action to apply this plan:".into());
    lines.push(format!("  macc o2 provider-apply {id} {checksum}"));
    lines.join("\n")
}

fn format_provider_apply(v: &Value) -> String {
    let outcome = v["outcome"].as_str().unwrap_or("unknown");
    if outcome != "supported" {
        return format!(
            "Provider Apply ({outcome}): {} — {}",
            val_str(&v["data"]["capability"]),
            val_str(&v["data"]["reason"])
        );
    }
    let d = &v["data"];
    format!(
        "Provider Applied:\n  Provider: {} v{}\n  State:    {}\n  Index:    {}\n  Revision: {}",
        d["provider_id"].as_str().unwrap_or("?"),
        d["version"].as_str().unwrap_or("?"),
        d["state"].as_str().unwrap_or("?"),
        val_str(&d["index"]),
        val_str(&d["source_revision"])
    )
}

fn format_providers(v: &Value) -> String {
    let mut lines = vec!["MACC Code Intelligence Providers".to_string()];
    if let Some(providers) = v["providers"].as_array() {
        for p in providers {
            let desc = &p["provider"];
            lines.push(format!(
                "  - {} v{} [status: {}] ({})",
                desc["id"].as_str().unwrap_or("?"),
                desc["version"].as_str().unwrap_or("?"),
                p["status"].as_str().unwrap_or("?"),
                desc["origin"].as_str().unwrap_or("?")
            ));
            if let Some(diags) = p["diagnostics"].as_array().filter(|a| !a.is_empty()) {
                for d in diags {
                    lines.push(format!("      * {}", val_str(d)));
                }
            }
        }
    }
    if let Some(receipts) = v["receipts"].as_array().filter(|a| !a.is_empty()) {
        lines.push(String::new());
        lines.push(format!("Deployed Receipts ({}):", receipts.len()));
        for r in receipts {
            lines.push(format!(
                "  - {} v{} (state: {}, index: {})",
                r["provider_id"].as_str().unwrap_or("?"),
                r["version"].as_str().unwrap_or("?"),
                r["state"].as_str().unwrap_or("?"),
                val_str(&r["index"])
            ));
        }
    }
    lines.join("\n")
}

fn format_agent_status(v: &Value) -> String {
    let mut lines = vec![
        format!(
            "MACC O2 Agent MCP Status: {}",
            if v["enabled"].as_bool().unwrap_or(false) {
                "ENABLED"
            } else {
                "DISABLED"
            }
        ),
        format!(
            "  Transport:        {} (protocol {})",
            val_str(&v["transport"]),
            val_str(&v["protocol"])
        ),
        format!("  Phase extraction: {}", val_str(&v["phase_extraction"])),
        format!("  Agent proposals:  {}", val_str(&v["agent_proposals"])),
    ];
    if let Some(tools) = v["tools"].as_array() {
        let names: Vec<_> = tools.iter().filter_map(|t| t.as_str()).collect();
        lines.push(format!("  Exposed tools:    {}", names.join(", ")));
    }
    if let Some(states) = v["tool_states"].as_array().filter(|a| !a.is_empty()) {
        lines.push(String::new());
        lines.push("Tool States:".into());
        for s in states {
            lines.push(format!(
                "  - {}: configured={} verified={} available_in_run={} consulted={}",
                s["tool_id"]
                    .as_str()
                    .or_else(|| s["tool"].as_str())
                    .unwrap_or("?"),
                val_str(&s["configured"]),
                val_str(&s["connection_verified"]),
                val_str(&s["available_in_run"]),
                val_str(&s["consulted"])
            ));
        }
    }
    if let Some(notice) = v["notice"].as_str() {
        lines.push(String::new());
        lines.push(format!("Note: {notice}"));
    }
    lines.join("\n")
}

fn format_semantic_status(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn format_compaction_list(v: &Value) -> String {
    let Some(rows) = v.as_array() else {
        return serde_json::to_string_pretty(v).unwrap_or_default();
    };
    let mut lines = vec![format!("MACC Compaction Probes ({}):", rows.len())];
    for r in rows {
        lines.push(format!(
            "  - {:<10} state={:<22} native_auto={} version={}",
            r["tool_id"].as_str().unwrap_or("?"),
            r["state"].as_str().unwrap_or("?"),
            r["native_auto"].as_str().unwrap_or("?"),
            r["version"].as_str().unwrap_or("—")
        ));
    }
    lines.join("\n")
}

fn format_compaction_probe(v: &Value) -> String {
    format!(
        "Compaction Probe ({}):\n  State:             {}\n  Command:           {}\n  Version:           {}\n  Native auto:       {}\n  Explicit headless: {}",
        v["tool_id"].as_str().unwrap_or("?"),
        v["state"].as_str().unwrap_or("?"),
        val_str(&v["command_path"]),
        val_str(&v["version"]),
        v["native_auto"].as_str().unwrap_or("?"),
        v["explicit_headless"].as_str().unwrap_or("?")
    )
}

fn format_context_list(v: &Value) -> String {
    let Some(rows) = v.as_array() else {
        return serde_json::to_string_pretty(v).unwrap_or_default();
    };
    let mut lines = vec![format!("MACC Context Manifests ({}):", rows.len())];
    if rows.is_empty() {
        lines.push("  (no context bundles recorded)".into());
    } else {
        for r in rows.iter().take(25) {
            lines.push(format!(
                "  - task={} phase={} role={} tokens={}/{} memories={} (id={})",
                r["task_id"].as_str().unwrap_or("?"),
                r["phase"].as_str().unwrap_or("?"),
                r["role"].as_str().unwrap_or("?"),
                val_str(&r["optional_estimated_tokens"]),
                val_str(&r["optional_limit_tokens"]),
                r["selected"].as_array().map(|a| a.len()).unwrap_or(0),
                r["id"].as_str().unwrap_or("?")
            ));
        }
    }
    lines.join("\n")
}

fn format_context_get(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn format_memory_search(v: &Value) -> String {
    if let Some(rows) = v.as_array() {
        let mut lines = vec![format!("Memories ({}):", rows.len())];
        for r in rows {
            let m = &r["memory"];
            lines.push(format!(
                "  - [{}] {} (id={}, scope={}, kind={}, score={})",
                m["status"].as_str().unwrap_or("?"),
                m["proposal"]["title"].as_str().unwrap_or("?"),
                m["id"].as_str().unwrap_or("?"),
                m["proposal"]["scope"].as_str().unwrap_or("?"),
                m["proposal"]["kind"].as_str().unwrap_or("?"),
                val_str(&r["score"])
            ));
        }
        return lines.join("\n");
    }
    if let Some(records) = v["records"].as_array() {
        let mut lines = vec![format!("Memory Search Results ({}):", records.len())];
        for r in records {
            let m = if r["memory"].is_object() {
                &r["memory"]
            } else {
                r
            };
            let title = m["proposal"]["title"]
                .as_str()
                .or_else(|| r["title"].as_str())
                .unwrap_or("?");
            let id = m["id"].as_str().or_else(|| r["id"].as_str()).unwrap_or("?");
            let status = m["status"]
                .as_str()
                .or_else(|| r["status"].as_str())
                .unwrap_or("active");
            lines.push(format!("  - [{status}] {title} (id={id})"));
        }
        if let Some(excl) = v["exclusions"].as_array().filter(|a| !a.is_empty()) {
            lines.push(format!("Exclusions ({}):", excl.len()));
            for e in excl {
                lines.push(format!("  * {}", val_str(e)));
            }
        }
        return lines.join("\n");
    }
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn format_memory_conflicts(v: &Value) -> String {
    let Some(rows) = v.as_array() else {
        return serde_json::to_string_pretty(v).unwrap_or_default();
    };
    let mut lines = vec![format!("Memory Conflicts ({}):", rows.len())];
    if rows.is_empty() {
        lines.push("  (no conflicts)".into());
    } else {
        for r in rows {
            lines.push(format!(
                "  - [{}] id={} left={} right={} reason={}",
                r["status"].as_str().unwrap_or("?"),
                r["id"].as_str().unwrap_or("?"),
                r["left_id"].as_str().unwrap_or("?"),
                r["right_id"].as_str().unwrap_or("?"),
                r["reason"].as_str().unwrap_or("?")
            ));
        }
    }
    lines.join("\n")
}
