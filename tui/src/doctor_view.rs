//! Project checks and operator actions tied to a concrete finding.
use super::*;
pub(super) fn cooldown(s: &AppState) -> Option<&state::ThrottledToolInfo> {
    s.coordinator_throttled_tools
        .get(s.inspector_index.checked_sub(s.doctor_findings.len())?)
}
pub(super) fn draw(f: &mut Frame, s: &AppState, area: Rect) {
    let (list, inspector) = workspace_ui::columns(area);
    let total = s.doctor_findings.len() + s.coordinator_throttled_tools.len();
    if total == 0 {
        f.render_widget(
            wrapped_paragraph(
                if s.doctor_job.is_some() {
                    "Running project checks…"
                } else {
                    "Run checks to inspect project readiness."
                },
                "Checks",
            ),
            area,
        );
        return;
    }
    let index = s.inspector_index.min(total - 1);
    let rows = s
        .doctor_findings
        .iter()
        .map(|r| vec![status(&r.severity).into(), r.title.clone()])
        .chain(
            s.coordinator_throttled_tools
                .iter()
                .map(|q| vec!["▲ Waiting".into(), format!("{} cooldown", q.tool_id)]),
        )
        .collect();
    workspace_ui::table(
        f,
        list,
        if s.doctor_job.is_some() {
            "Checks · refreshing"
        } else {
            "Checks"
        },
        &["Status", "Check"],
        rows,
        Some(index),
        crate::navigation::Mode::Operate,
    );
    let detail = if let Some(row) = s.doctor_findings.get(index) {
        if s.doctor_raw {
            serde_json::to_string_pretty(row).unwrap_or_default()
        } else {
            [
                Some(row.title.as_str()),
                Some(row.message.as_str()),
                row.recommended_action.as_deref(),
            ]
            .into_iter()
            .flatten()
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
        }
    } else {
        let q = &s.coordinator_throttled_tools[index - s.doctor_findings.len()];
        if s.doctor_raw {
            format!(
                "Tool   {}\nExpires   {}\nBackoff   {} s\nConsecutive 429 responses   {}",
                q.tool_id, q.throttled_until, q.backoff_seconds, q.consecutive_count
            )
        } else {
            format!("{} cooldown\n\nAvailable at {}.\n\n[Enter] Clear MACC's local wait and allow dispatch. The provider's quota still applies.",q.tool_id,q.display_until)
        }
    };
    workspace_ui::details(
        f,
        inspector,
        if s.doctor_raw {
            "Evidence"
        } else {
            "Inspector"
        },
        detail,
        s.inspector_scroll,
    );
}

pub(super) fn status(severity: &macc_core::doctor::DiagnosticSeverity) -> &'static str {
    use macc_core::doctor::DiagnosticSeverity;
    match severity {
        DiagnosticSeverity::Ok => "✓ Verified",
        DiagnosticSeverity::Info => "ℹ Info",
        DiagnosticSeverity::Warning => "▲ Warning",
        DiagnosticSeverity::Error => "✕ Error",
    }
}
