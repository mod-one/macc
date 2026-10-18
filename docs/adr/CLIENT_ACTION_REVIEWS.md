# Shared graphical action reviews

## Context

CLI confirmation and preflight decisions were bypassed by TUI/Web entry points. TUI source materialization printed into the alternate screen. Diagnostics and onboarding disagreed about PRD Queue selection.

## Decision

Keep review data and execution checks in core services; keep rendering and input handling in clients. Add three narrow Engine operations for reading a launch review, executing a confirmed decision, and attaching the selected supervisor. The Engine surface guard intentionally changes from 143 to 146 methods. No provider names or provider-specific launch logic enter the clients.

Share the existing CLI supervisor bootstrap with the graphical launch service. Revalidate launch conditions and ownership immediately before action. Bind Apply confirmation to the exact action plan and before/after file contents. Preserve CLI unattended flags and project-only Web apply compatibility.

TUI state/rendering moves into dedicated Apply and launch modules instead of growing the composition roots. The TUI's Engine handle now requires Send + Sync, matching the existing CLI/Web shared-engine contract, so launch readiness can wait off the render thread. The Engine trait itself keeps its existing bounds.

Use one read-only PRD Queue resolver for diagnostics, onboarding and launch reviews. Preserve saved progress and report mismatches; never reset it implicitly.

## Consequences

New Web Run callers must obtain and confirm a review. Existing explicit Apply callers can omit the optional plan revision, while graphical clients always submit it. Test engines can override the narrow launch operations; production continues through existing managed-process and ownership services. Confirmation windows are presentation, while execution guards remain authoritative.
