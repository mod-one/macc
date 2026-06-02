/// Automatic model selection engine for coordinator task dispatch (spec §8–§11).
///
/// Reads `routing_hints` from a PRD task's `extra` map, applies phase-based
/// defaults, and produces a `RoutingDecision` that specifies which model tier
/// and reasoning depth to use.
///
/// Clients must never hardcode provider model names.  This module is
/// provider-neutral: it emits symbolic tiers ("mini", "standard", "heavy")
/// that tool adapters map to concrete model IDs.
use crate::coordinator::model::Task;
use crate::config::ModelRoutingConfig;
use serde::{Deserialize, Serialize};

// ── Public types ──────────────────────────────────────────────────────────────

/// Symbolic model tier resolved by the routing engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Mini,
    Standard,
    Heavy,
}

impl ModelTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelTier::Mini => "mini",
            ModelTier::Standard => "standard",
            ModelTier::Heavy => "heavy",
        }
    }
}

/// Symbolic reasoning depth resolved by the routing engine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningDepth {
    Light,
    Standard,
    Deep,
}

impl ReasoningDepth {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReasoningDepth::Light => "light",
            ReasoningDepth::Standard => "standard",
            ReasoningDepth::Deep => "deep",
        }
    }
}

/// The outcome of the routing engine for one task/phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    /// Symbolic model tier to use.
    pub tier: ModelTier,
    /// Reasoning depth to use.
    pub reasoning_depth: ReasoningDepth,
    /// Whether routing operated in auto or manual mode.
    pub mode: String,
    /// Human-readable reasons for this decision (for logging/observability).
    pub reasons: Vec<String>,
}

impl RoutingDecision {
    /// Compact log line: `Auto -> standard / standard (reason1, reason2)`.
    pub fn summary(&self) -> String {
        let mode = if self.mode == "auto" { "Auto" } else { "Manual" };
        format!(
            "{} -> {} / {} reasoning ({})",
            mode,
            self.tier.as_str(),
            self.reasoning_depth.as_str(),
            if self.reasons.is_empty() {
                "defaults".to_string()
            } else {
                self.reasons.join(", ")
            }
        )
    }
}

// ── Routing hints extracted from a task ──────────────────────────────────────

/// Parsed routing hints from a PRD task's `routing_hints` object.
/// All fields are optional — missing values fall back to phase defaults.
#[derive(Debug, Clone, Default)]
struct RoutingHints {
    execution_mode: Option<String>,    // micro | standard | structural
    reasoning_depth: Option<String>,   // light | standard | deep
    context_scope: Option<String>,     // local | module | cross-cutting
    risk_level: Option<String>,        // low | medium | high
    validation_profile: Option<String>, // light | standard | heavy
}

fn extract_routing_hints(task: &Task) -> RoutingHints {
    let Some(hints_val) = task.extra.get("routing_hints") else {
        return RoutingHints::default();
    };
    let Some(obj) = hints_val.as_object() else {
        return RoutingHints::default();
    };
    let get = |k: &str| obj.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    RoutingHints {
        execution_mode: get("execution_mode"),
        reasoning_depth: get("reasoning_depth"),
        context_scope: get("context_scope"),
        risk_level: get("risk_level"),
        validation_profile: get("validation_profile"),
    }
}

// ── Phase defaults (spec §8) ──────────────────────────────────────────────────

fn phase_defaults(phase: &str) -> (ModelTier, ReasoningDepth) {
    match phase {
        "exploration" | "triage" | "summarization" | "log_reading" | "light_review" => {
            (ModelTier::Mini, ReasoningDepth::Light)
        }
        "architecture" | "deep_refactor" => (ModelTier::Heavy, ReasoningDepth::Deep),
        // prd_generation, implementation, review, fix, merge_fix, testing, validation
        _ => (ModelTier::Standard, ReasoningDepth::Standard),
    }
}

// ── Tier ordering (for escalation logic) ─────────────────────────────────────

fn tier_rank(t: &ModelTier) -> u8 {
    match t { ModelTier::Mini => 0, ModelTier::Standard => 1, ModelTier::Heavy => 2 }
}

fn depth_rank(d: &ReasoningDepth) -> u8 {
    match d { ReasoningDepth::Light => 0, ReasoningDepth::Standard => 1, ReasoningDepth::Deep => 2 }
}

fn max_tier(a: ModelTier, b: ModelTier) -> ModelTier {
    if tier_rank(&a) >= tier_rank(&b) { a } else { b }
}

fn max_depth(a: ReasoningDepth, b: ReasoningDepth) -> ReasoningDepth {
    if depth_rank(&a) >= depth_rank(&b) { a } else { b }
}

fn min_tier(a: ModelTier, b: ModelTier) -> ModelTier {
    if tier_rank(&a) <= tier_rank(&b) { a } else { b }
}

fn min_depth(a: ReasoningDepth, b: ReasoningDepth) -> ReasoningDepth {
    if depth_rank(&a) <= depth_rank(&b) { a } else { b }
}

// ── Core routing function ─────────────────────────────────────────────────────

/// Compute the model routing decision for a task being dispatched.
///
/// `phase` is the current coordinator phase (e.g. "implementation", "review",
/// "fix", "architecture").  When `routing_config` is `None` or the mode is
/// `manual`, the decision is `Standard / Standard` unless overridden.
///
/// Never blocks a task dispatch: if routing_hints are absent or malformed,
/// the function returns the phase default (standard for most phases).
pub fn decide(
    task: &Task,
    phase: &str,
    routing_config: Option<&ModelRoutingConfig>,
) -> RoutingDecision {
    let mode_str = routing_config
        .map(|c| c.mode.as_str())
        .unwrap_or("auto");

    // Manual mode: return Standard/Standard (or config-specified manual defaults).
    if mode_str == "manual" {
        let (default_tier, default_depth) = manual_defaults(routing_config);
        return RoutingDecision {
            tier: default_tier,
            reasoning_depth: default_depth,
            mode: "manual".into(),
            reasons: vec!["manual mode".into()],
        };
    }

    // Auto mode.
    let (mut tier, mut depth) = phase_defaults(phase);
    let mut reasons: Vec<String> = vec![format!("phase={}", phase)];

    let hints = extract_routing_hints(task);

    // Apply execution_mode hint.
    match hints.execution_mode.as_deref() {
        Some("structural") => {
            tier = max_tier(tier, ModelTier::Heavy);
            depth = max_depth(depth, ReasoningDepth::Deep);
            reasons.push("execution_mode=structural".into());
        }
        Some("micro") => {
            tier = min_tier(tier, ModelTier::Mini);
            depth = min_depth(depth, ReasoningDepth::Light);
            reasons.push("execution_mode=micro".into());
        }
        _ => {}
    }

    // Apply context_scope hint.
    match hints.context_scope.as_deref() {
        Some("cross-cutting") => {
            tier = max_tier(tier, ModelTier::Heavy);
            depth = max_depth(depth, ReasoningDepth::Deep);
            reasons.push("context_scope=cross-cutting".into());
        }
        Some("local") => {
            // Can allow downgrade if execution_mode is also micro
            if hints.execution_mode.as_deref() == Some("micro") {
                tier = min_tier(tier, ModelTier::Mini);
            }
        }
        _ => {}
    }

    // Apply risk_level hint.
    match hints.risk_level.as_deref() {
        Some("high") => {
            tier = max_tier(tier, ModelTier::Heavy);
            depth = max_depth(depth, ReasoningDepth::Deep);
            reasons.push("risk_level=high".into());
        }
        Some("low") => {
            // Permit downgrade only if nothing else escalated.
            if tier == ModelTier::Standard && depth == ReasoningDepth::Standard {
                tier = ModelTier::Mini;
                depth = ReasoningDepth::Light;
                reasons.push("risk_level=low".into());
            }
        }
        _ => {}
    }

    // Apply validation_profile hint.
    match hints.validation_profile.as_deref() {
        Some("heavy") => {
            tier = max_tier(tier, ModelTier::Heavy);
            depth = max_depth(depth, ReasoningDepth::Deep);
            reasons.push("validation_profile=heavy".into());
        }
        Some("light") => {
            // Only downgrade reasoning, not tier (validation is a quality gate).
            depth = min_depth(depth, ReasoningDepth::Light);
            reasons.push("validation_profile=light".into());
        }
        _ => {}
    }

    // Apply explicit reasoning_depth hint (highest priority, overrides above).
    match hints.reasoning_depth.as_deref() {
        Some("deep") => {
            depth = max_depth(depth, ReasoningDepth::Deep);
            // Deep reasoning requires at least Standard tier.
            tier = max_tier(tier, ModelTier::Standard);
            reasons.push("reasoning_depth=deep".into());
        }
        Some("light") => {
            depth = min_depth(depth, ReasoningDepth::Light);
            reasons.push("reasoning_depth=light".into());
        }
        Some("standard") => {
            // Explicit standard: clamp depth but do not escalate tier.
            depth = ReasoningDepth::Standard;
        }
        _ => {}
    }

    // Apply global auto-config overrides (escalation policy).
    if let Some(auto_cfg) = routing_config.and_then(|c| c.auto.as_ref()) {
        // prefer_mini_under_budget_pressure: if tier is Standard with no
        // escalation triggers, downgrade to Mini when budget pressure is set.
        // (Budget pressure signal is not yet tracked; this is a placeholder.)
        if auto_cfg.prefer_mini_under_budget_pressure
            && tier == ModelTier::Standard
            && reasons.len() == 1
        {
            // Only downgrade if the sole reason is the phase default and phase
            // allows it (i.e. is not a high-stakes phase).
            if !matches!(phase, "review" | "architecture" | "deep_refactor") {
                tier = ModelTier::Mini;
                depth = ReasoningDepth::Light;
                reasons.push("prefer_mini_budget_pressure".into());
            }
        }
    }

    RoutingDecision {
        tier,
        reasoning_depth: depth,
        mode: "auto".into(),
        reasons,
    }
}

fn manual_defaults(cfg: Option<&ModelRoutingConfig>) -> (ModelTier, ReasoningDepth) {
    let manual = cfg.and_then(|c| c.manual.as_ref());
    let tier = manual
        .and_then(|m| m.default_model.as_deref())
        .and_then(|m| tier_from_str(m))
        .unwrap_or(ModelTier::Standard);
    let depth = manual
        .and_then(|m| m.default_reasoning_depth.as_deref())
        .and_then(|d| depth_from_str(d))
        .unwrap_or(ReasoningDepth::Standard);
    (tier, depth)
}

fn tier_from_str(s: &str) -> Option<ModelTier> {
    match s {
        "mini" => Some(ModelTier::Mini),
        "standard" => Some(ModelTier::Standard),
        "heavy" => Some(ModelTier::Heavy),
        _ => None,
    }
}

fn depth_from_str(s: &str) -> Option<ReasoningDepth> {
    match s {
        "light" => Some(ReasoningDepth::Light),
        "standard" => Some(ReasoningDepth::Standard),
        "deep" => Some(ReasoningDepth::Deep),
        _ => None,
    }
}

// Expose as_str on ModelRoutingMode for convenience.
impl crate::config::ModelRoutingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            crate::config::ModelRoutingMode::Auto => "auto",
            crate::config::ModelRoutingMode::Manual => "manual",
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coordinator::model::Task;
    use serde_json::json;

    fn task_with_hints(hints: serde_json::Value) -> Task {
        let mut extra = std::collections::BTreeMap::new();
        extra.insert("routing_hints".to_string(), hints);
        Task { extra, ..Task::default() }
    }

    fn task_no_hints() -> Task {
        Task::default()
    }

    #[test]
    fn no_hints_defaults_to_standard() {
        let decision = decide(&task_no_hints(), "implementation", None);
        assert_eq!(decision.tier, ModelTier::Standard);
        assert_eq!(decision.reasoning_depth, ReasoningDepth::Standard);
        assert_eq!(decision.mode, "auto");
    }

    #[test]
    fn no_hints_review_phase_standard() {
        let decision = decide(&task_no_hints(), "review", None);
        assert_eq!(decision.tier, ModelTier::Standard);
        assert_eq!(decision.reasoning_depth, ReasoningDepth::Standard);
    }

    #[test]
    fn no_hints_architecture_escalates() {
        let decision = decide(&task_no_hints(), "architecture", None);
        assert_eq!(decision.tier, ModelTier::Heavy);
        assert_eq!(decision.reasoning_depth, ReasoningDepth::Deep);
    }

    #[test]
    fn structural_execution_escalates_to_heavy() {
        let task = task_with_hints(json!({ "execution_mode": "structural" }));
        let decision = decide(&task, "implementation", None);
        assert_eq!(decision.tier, ModelTier::Heavy);
        assert_eq!(decision.reasoning_depth, ReasoningDepth::Deep);
        assert!(decision.reasons.iter().any(|r| r.contains("structural")));
    }

    #[test]
    fn micro_execution_downgrades() {
        let task = task_with_hints(json!({ "execution_mode": "micro" }));
        let decision = decide(&task, "implementation", None);
        assert_eq!(decision.tier, ModelTier::Mini);
        assert_eq!(decision.reasoning_depth, ReasoningDepth::Light);
    }

    #[test]
    fn high_risk_escalates() {
        let task = task_with_hints(json!({ "risk_level": "high" }));
        let decision = decide(&task, "implementation", None);
        assert_eq!(decision.tier, ModelTier::Heavy);
        assert_eq!(decision.reasoning_depth, ReasoningDepth::Deep);
    }

    #[test]
    fn cross_cutting_scope_escalates() {
        let task = task_with_hints(json!({ "context_scope": "cross-cutting" }));
        let decision = decide(&task, "implementation", None);
        assert_eq!(decision.tier, ModelTier::Heavy);
    }

    #[test]
    fn heavy_validation_escalates() {
        let task = task_with_hints(json!({ "validation_profile": "heavy" }));
        let decision = decide(&task, "review", None);
        assert_eq!(decision.tier, ModelTier::Heavy);
    }

    #[test]
    fn explicit_deep_reasoning_upgrades_tier() {
        let task = task_with_hints(json!({ "reasoning_depth": "deep" }));
        let decision = decide(&task, "summarization", None); // mini by default
        // deep reasoning requires at least Standard tier
        assert!(tier_rank(&decision.tier) >= tier_rank(&ModelTier::Standard));
        assert_eq!(decision.reasoning_depth, ReasoningDepth::Deep);
    }

    #[test]
    fn missing_hints_never_blocks_dispatch() {
        // No panic, no error — always returns a valid decision.
        let task = Task { extra: Default::default(), ..Task::default() };
        let decision = decide(&task, "implementation", None);
        assert_eq!(decision.tier, ModelTier::Standard);
    }

    #[test]
    fn malformed_hints_falls_back_gracefully() {
        // hints is not an object — should fall back to phase defaults.
        let mut extra = std::collections::BTreeMap::new();
        extra.insert("routing_hints".to_string(), json!("not-an-object"));
        let task = Task { extra, ..Task::default() };
        let decision = decide(&task, "implementation", None);
        assert_eq!(decision.tier, ModelTier::Standard);
    }

    #[test]
    fn manual_mode_returns_standard_regardless_of_hints() {
        let task = task_with_hints(json!({
            "execution_mode": "structural",
            "risk_level": "high",
        }));
        let cfg = crate::config::ModelRoutingConfig {
            mode: crate::config::ModelRoutingMode::Manual,
            ..Default::default()
        };
        let decision = decide(&task, "implementation", Some(&cfg));
        assert_eq!(decision.mode, "manual");
        assert_eq!(decision.tier, ModelTier::Standard);
    }
}
