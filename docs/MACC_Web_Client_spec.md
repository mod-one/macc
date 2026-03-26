# MACC Web Client (Option A) — Unified Specification (Local Web UI)

## 0. Document Info
- Product: **MACC** (Multi-Assistant Code Config)
- Component: **Local Web Client (SPA)** + **Local Web API** (served by `macc`)
- Mode: **Local-first**, offline-capable, served from a **single binary**
- Primary compatibility goal: **feature parity** with CLI/TUI via the same core engine (resolve → plan → apply), plus web-only observability and editing features.

---

## 1. Background and Goals

### 1.1 Why a Web UI
MACC already provides CLI/TUI flows. The local Web UI adds:
- better observability (live streams, dashboards, multi-pane views),
- safer change review (rich diffs, backups visualization),
- faster editing (schema-driven editors, PRD tooling),
- onboarding (wizard + guided walkthrough).

### 1.2 Primary Goals (v1)
1) Expose the **same MACC engine** as CLI/TUI for:
   - canonical config editing,
   - plan/preview/diff,
   - apply with backups and consent gates,
   - worktrees orchestration,
   - coordinator monitoring and controls,
   - logs and diagnostics.

2) Improve operator experience:
   - real-time events + log streaming,
   - multi-worktree “live wall”,
   - runtime metrics + system load,
   - quick actions with safety prompts.

3) Add web-specific productivity:
   - interactive PRD editor (`prd.json`) with validation + graph,
   - embedded terminals (project + worktree),
   - unified search, notifications, guided onboarding.

### 1.3 Non-Goals (v1)
- No remote/public hosting (localhost only by default).
- No multi-user auth system (single local operator, token protected).
- No execution of post-install scripts from downloaded packages.
- No replacement of the AI tools themselves.
- No “CI/CD orchestrator” beyond MACC coordinator responsibilities.

---

## 2. Terminology (Unified)
- **Project**: a git repository where `.macc/` exists.
- **Canonical config**: `.macc/macc.yaml` (source of truth).
- **Tool / Adapter**: a tool integration (Claude/Codex/Gemini/…) plus its adapter settings.
- **Skill**: packaged behavior/instructions installed per tool.
- **Agent**: packaged role/persona definition installed per tool (or a fallback if tool doesn’t support agent files).
- **MCP server**: an external capability definition merged into project/user config (no secrets stored).
- **ActionPlan**: the plan result (files to write + diffs + required consents).
- **Worktree**: a git worktree used as an isolated execution slot.
- **Performer**: a worktree-local executor (runs the tool runner and writes logs).
- **Coordinator**: the control plane that dispatches tasks, monitors performers, and converges the system.
- **Registry**: coordinator state store for tasks and runtime statuses.
- **Doctor**: diagnostics framework and safe auto-fix capability.

---

## 3. System Architecture (Option A)

### 3.1 Runtime Components
- `macc` (Rust):
  - core engine: config/resolve/plan/apply/worktrees/coordinator/doctor
  - web server:
    - REST API (JSON)
    - static asset server (SPA)
    - streaming endpoints (SSE and/or WebSocket)
    - terminal gateway (PTY over WebSocket)

- Web Client (SPA):
  - served by `macc`
  - communicates only with the local API (`http://127.0.0.1:<port>`)

### 3.2 Binding, Port and Startup UX
- Default bind: `127.0.0.1`
- Default port: `3450` (configurable via `settings.web_port` and CLI flags)
- Startup prints:
  - URL
  - auth token (or token hint path)
  - quick hints (open browser, reset token)

### 3.3 Data Sources (Canonical)
- Config: `.macc/macc.yaml`
- PRD:
  - `prd.json` (project root)
  - `worktree.prd.json` (optional, per worktree)
- Logs:
  - `.macc/log/coordinator/`
  - `.macc/log/performer/`
- Coordinator state:
  - registry JSON/SQLite (implementation choice)
  - events JSONL
- Worktrees:
  - `.macc/worktree.json` in each worktree (metadata)
  - git worktree list
- Backups: `.macc/backups/<timestamp>/...`
- Tool sessions: `.macc/state/tool-sessions.json`
- Cache: `.macc/cache/` (skills/MCP artifacts)

---

## 4. Security, Privacy, and Safety Model

### 4.1 Local Security Defaults
- localhost binding by default (no external exposure)
- token required for:
  - all API requests,
  - SSE/WebSocket,
  - terminal sessions.

### 4.2 Consent Gates and Risk Levels
All write actions must be classified:
- **Safe**: read-only operations, scans, viewing logs.
- **Caution**: project-level writes (e.g., apply config, create worktree).
- **Dangerous**: destructive operations (remove worktree, cleanup, emergency stop).
- **User-level writes**: always require explicit consent + backups.

UI requirements:
- show diff/summary before any apply or restore
- confirmations:
  - Caution: single confirm
  - Dangerous: double confirm or typed phrase

### 4.3 File System Boundaries
- API must not read/write outside the project root,
  except user-level config operations that are explicitly enabled and consented.
- Terminals must be constrained to:
  - project root
  - selected worktree paths

### 4.4 Secrets Handling
- No secrets are ever written into repo outputs.
- UI must redact likely secrets in logs/diffs (best effort).
- MCP env values must be placeholders (e.g., `${ENV_VAR}`).

### 4.5 Package Safety (Skills/MCP)
- Remote packages must be **data-only** and include `macc.package.json`.
- No post-install scripts; no executing downloaded code.
- Install review UI must show permissions/risks (env/network/fs).

### 4.6 Ops Audit Log
- All operator actions are appended to `.macc/log/ops.jsonl`:
  - who/what/when (local user), action, inputs, result summary, links to logs.

---

## 5. UX System (Coherent)

### 5.1 App Shell (Global Layout)
- Left sidebar navigation (collapsible)
- Top bar:
  - project selector / repo path
  - global search and command palette
  - connection indicators (API + streams)
  - primary CTA (contextual): Run/Start Task
- Bottom status strip (always visible):
  - CPU/RAM/Disk (project + cache)
  - coordinator state
  - active workers
  - throttled tools summary (if any)
  - notifications summary

### 5.2 Modes (Single Mental Model)
The UI uses two consistent modes:
1) **Setup & Config Mode**
   - onboarding, tools/adapters, standards, skills/agents/MCP, plan/apply, PRD editing
2) **Ops Mode**
   - coordinator console, registry, live wall, diagnostics, locks graph, runtime controls

Mode switching:
- preserves selected project/worktree/task context
- preserves user layout preferences (panels open/closed)

### 5.3 Visual Design System
- Default theme: dark, console-grade readability
- Cards: consistent radius, subtle border, soft shadow
- Typography:
  - UI font for navigation and cards
  - monospace for logs, paths, hashes
- Density toggle: Comfortable / Compact
- Status semantics:
  - severity color + icon + text label (never color only)
- Reduced motion support required

### 5.4 Motion & Micro-Interactions
- active stream pulse indicator
- KPI count transitions (short easing)
- log streaming:
  - new-line highlight fade
  - “new lines” badge when paused
- drawers/modals slide-in / cross-fade
- graph selection glow and edge highlighting

### 5.5 Global Components (Inventory)
- KPI Card
- Issue Card (Doctor)
- Stream Tile (Performer/Worker)
- Task List Item (Registry)
- Right Drawer Inspector (schema-driven editor)
- Command Palette (Ctrl+K)
- Notifications Drawer (bell)
- Terminal Drawer (tabs)

---

## 6. Navigation and Routes (Recommended)
> Actual paths are implementation choices; these routes define the IA.

### 6.1 Setup & Config Mode
- `/welcome` — Welcome / Quick Start
- `/init` — Project Initialization Wizard
- `/dashboard` — Overview
- `/config/tools` — Tools & Adapters
- `/config/standards` — Standards
- `/config/skills` — Skills / Agents / MCP Catalog
- `/config/settings` — Settings (General / Coordinator / Advanced)
- `/prd` — PRD Editor (Table / Detail / Graph / Diff)
- `/plan` — Plan
- `/apply` — Apply

### 6.2 Ops Mode
- `/ops/console` — Coordinator Console (KPI + streams + registry)
- `/ops/registry` — Registry & Task Inspector
- `/ops/live` — Live Wall (multi-stream)
- `/ops/locks` — Dependency Graph & Locks
- `/ops/diagnostics` — Doctor (system + categories)
- `/ops/logs` — Logs Explorer + Event Timeline
- `/ops/backups` — Backups & Restore

### 6.3 Shared / Utility
- `/help` — In-app docs viewer
- `/about` — version, paths, support info

---

## 7. Core Pages and Functional Requirements (Consolidated)

## 7.1 Welcome / Quick Start
Purpose: first-run landing and operator hub.
- 3 primary cards:
  1) Detect & Install adapters
  2) Configure Project (open wizard)
  3) Import Skills (open catalog)
- CTAs:
  - Quick Start
  - Start Automated Walkthrough (guided tour)
- “New version available” badge (non-blocking; respects offline mode)

## 7.2 Project Initialization Wizard (Parity with `macc init --wizard`)
4 steps (stepper + progress bar):
1) Welcome & Select Project Root
2) Tool Detection (preview + toggles)
3) Standards (preset + preview)
4) Review (show the config preview, optional plan preview)
Completion:
- creates `.macc/`, writes `.macc/macc.yaml`,
- optional initial plan and apply (with consent).

## 7.3 Dashboard (Overview)
Must show:
- Project summary (path, branch, dirty state, MACC version)
- Coordinator summary (Running/Paused/Idle, current loop phase)
- Worktrees summary (total/active/idle/stale/dirty)
- Recent activity (last plan/apply, last errors)
- Alerts (doctor, throttling, stale tasks)

## 7.4 Tools & Adapters (Cards + Right Drawer Editor)
- Grid of adapter cards:
  - name, version, category, capability chips
  - state: Enabled/Disabled + Healthy/Degraded + Idle/Active
- Header actions:
  - Check Updates
  - Add Adapter
- Filter row:
  - search
  - segmented filter: All / Enabled / Installed
- Right drawer (pin-able):
  - schema-driven sections:
    - container settings (image, cpu, mem)
    - mounts
    - network access toggle (risk labeled)
    - advanced: env, timeouts
  - raw view toggle (YAML/JSON) + copy
  - unsaved changes handling (apply/revert)

## 7.5 Standards
- Select preset + overrides
- Preview rendered outputs per tool (read-only)
- Diff from preset
- Lint warnings for inconsistent conventions

## 7.6 Skills / Agents / MCP
- Catalog browser:
  - search, filter by tool compatibility, verified status, source kind
  - add by URL (Git/HTTP/Local), pin revision/checksum
- Install flow:
  - Security Review modal (permissions + risks)
  - Configuration tab
  - Manifest tab (raw `macc.package.json`)
- Cache status:
  - cached/not cached
  - offline behavior: uses cache only

## 7.7 PRD Editor (`prd.json`)
Views:
- Table view (virtualized) + filters/sorts
- Task detail pane:
  - form editor for common fields
  - raw JSON editor
- Graph view:
  - dependency DAG, cycles detection, critical path
- Diff view:
  - unsaved vs saved
  - project PRD vs selected worktree PRD
Guardrails:
- validation required to save (or explicit “save anyway” with warnings)
- backups on save
- format on save (optional)

## 7.8 Plan
- Run plan with options:
  - scope: project / selected tools / selected worktrees
  - offline/quiet flags
- Display ActionPlan:
  - file list, diffs, risk levels, consent requirements
  - plan hash + config hash for reproducibility

## 7.9 Apply
- Requires explicit confirmation:
  - summary + diff
  - backup locations
- Modes:
  - apply now (project)
  - apply to selected worktrees
  - dry-run (no writes)
- Post-apply:
  - results summary + links to logs/backups

## 7.10 Worktrees (Unified “Management”)
Two display modes (toggle, persisted):
1) Table (operator view)
2) Cards (slot view)

Header:
- Filter (advanced builder), Export, Refresh
- Primary CTA: Create Worktree

KPI row:
- Total worktrees, active leases, dirty, stale

Bulk actions (side panel):
- Update All (apply across worktrees)
- Cleanup Stale (prune/remove with confirmation)
- Bulk selection actions: Apply / Doctor / Open Terminal / Remove

Row expansion (table):
- scope chips
- recent performer logs tail snippet
- quick actions: open, run, doctor, remove, copy path/branch

Worktree Card:
- name, branch, tool badge
- status: executing/idle/dirty/stale
- optional mini CPU/RAM widgets
- open in editor, open terminal, tail logs, remove (confirm)

Create Worktree Wizard:
- basics (slug/tool/count)
- base branch + scope
- review (paths, branches, files to write)
- create + optional auto-apply

## 7.11 Coordinator Console (Ops)
Console header:
- syncing + health + throttling chips
- emergency stop (always visible; destructive gating)
KPI row:
- status, elapsed time, queue, health %
Active performer streams:
- grid of Stream Tiles (live logs)
Task registry panel:
- list with states and priorities
Resources panel:
- CPU/RAM/Disk + throttling gauge (effective vs configured max_parallel)
Quick actions toolbar:
- stop, resume, refresh status, run full cycle
- sync registry, reconcile, unlock, cleanup, audit PRD

## 7.12 Registry & Task Inspector
Registry table:
- state, tool, attempts, last heartbeat, delayed_until
Task inspector:
- event history timeline
- linked commits (if available)
Operator actions:
- requeue
- mark abandoned (confirm)
- reassign tool (requires justification note)

## 7.13 Live Wall (Multi-Stream)
- grid/stack of all active worktree streams
- filters: active only, per tool, errors only
- per tile: pause, copy last N lines, download segment
- click to open fullscreen worker detail

## 7.14 Worker Detail (Fullscreen)
Tabs:
- live logs
- tool transcript
- events timeline
- artifacts (worktree.prd.json, diffs, generated files)
Controls:
- stop worker, restart phase, open worktree, open terminal
- “copy diagnostics bundle” (last N lines + key events)

## 7.15 Dependency Graph & Locks (Ops)
Graph shows:
- task dependencies
- exclusive resource contention
- tool session leases
- worktree slot allocation
Interactions:
- zoom/pan/reset
- node selection opens details drawer
Warnings:
- deadlock banner with suggested resolution shortcuts:
  reconcile → unlock → cleanup

## 7.16 Diagnostics (Doctor)
- Overview:
  - overall health score + delta
  - critical/warn/suggestion counts
  - tabs by severity
- Category sidebar with counts and health indicator
- Issue cards:
  - current vs expected state
  - code (e.g., WT-404)
  - safe fix/manual/ignore actions
- Bulk Fix (Safe):
  - preview list of fixes
  - confirmation + backups
  - post-fix summary + links

## 7.17 Logs Explorer + Events
- browse `.macc/log/` by category
- search within file
- timestamp jump
- download
- tail (live)
Structured events view:
- parse JSONL
- filter by worktree/task/phase/severity
- timeline visualization

## 7.18 Backups & Restore
- list backups by timestamp
- per backup:
  - file list
  - diff vs current
  - restore selected or full (confirm)
- safety:
  - warn on overwriting newer changes
  - create “restore backup” before restore

## 7.19 Embedded Terminals (Web)
Terminal types:
- project root
- per worktree
- optional read-only “log follow terminal”
Requirements:
- multi-tab sessions
- PTY over WebSocket
- session lifecycle: create/attach/detach/kill; idle cleanup
Security:
- token required
- directory restriction
Integration:
- “open terminal here” context actions
- optional performer output follow mode

## 7.20 Global Search, Command Palette, Notifications, Help
- Global search:
  - worktrees, tasks, logs, skills/MCP, settings keys
  - grouped results + keyboard nav
  - preview panel
- Command palette (Ctrl+K):
  - run plan/apply
  - create worktree
  - open PRD
  - coordinator actions
- Notifications center:
  - doctor alerts, coordinator pauses, apply results, updates available
- Help:
  - offline-friendly docs viewer
  - contextual help drawer per page

---

## 8. Realtime, Events, and Streaming

### 8.1 Channels
- coordinator events stream
- log tail streams
- terminal PTY streams
- long operation progress events (apply/fetch)

### 8.2 Transport
- SSE preferred for events + log tails
- WebSocket required for terminals and bi-directional controls

### 8.3 Client Behavior
- auto reconnect with backoff
- stream health indicators
- per stream pause/auto-scroll controls
- bounded buffers + truncation policy

---

## 9. API Contract (High-Level)

### 9.1 Core Endpoints (Examples)
- `GET  /api/v1/status`
- `GET  /api/v1/config` / `PUT /api/v1/config`
- `GET  /api/v1/prd` / `PUT /api/v1/prd`
- `POST /api/v1/plan`
- `POST /api/v1/apply`
- `GET  /api/v1/worktrees`
- `POST /api/v1/worktrees`
- `DELETE /api/v1/worktrees/{id}`
- `POST /api/v1/worktrees/{id}/run`
- `GET  /api/v1/coordinator/status`
- `POST /api/v1/coordinator/{action}`
- `GET  /api/v1/logs`
- `GET  /api/v1/logs/{path}`
- `GET  /api/v1/logs/tail?path=...` (SSE)
- `POST /api/v1/terminal`
- `WS   /api/v1/terminal/{session}`

### 9.2 Error Model
All errors return:
- `code` (stable string)
- `message` (operator readable)
- `retryable` (bool)
- `recommended_action` (string)
UI requirements:
- quick buttons: Retry, Copy diagnostics, Open logs

---

## 10. Performance and Reliability
- UI initial load < 2s on medium projects (cached assets)
- Virtualized tables for:
  - large PRD
  - large logs
- Streaming targets:
  - 20+ concurrent stream tiles (configurable)
  - bounded memory usage per stream (truncate)
- Apply pipeline should remain fast; downloads are cached.

---

## 11. Accessibility
- Full keyboard navigation
- Proper ARIA labels
- Color-agnostic status indicators (icons + text)
- Reduced motion support

---

## 12. Testing and Observability
- API contract tests
- Smoke UI tests (headless)
- Streaming reliability tests (reconnect/backoff)
- Golden tests for plan/apply diffs rendering (snapshot)
- Ops audit log must be validated in tests for dangerous actions

---

## 13. Acceptance Criteria (MVP)
1) `macc web` launches UI on localhost with token protection.
2) Config editor reads/writes `.macc/macc.yaml` with validation and backups.
3) Plan displays ActionPlan diffs without writing.
4) Apply writes atomically, creates backups, and enforces consent.
5) Worktrees page lists and manages worktrees; can open worktree details and live logs.
6) Coordinator console shows state, metrics, throttling, and can run actions (stop/resume/reconcile/unlock/cleanup).
7) Clicking an active worker opens a fullscreen live log view.
8) Multi-stream Live Wall works for active worktrees.
9) PRD editor supports table + detail + validation + save backups.
10) Project root and worktree terminals work, with restrictions.

---

## 14. Deliverables
- `macc-web` module integrated into `macc`:
  - REST API + SSE/WS streaming + terminal gateway
- Web SPA:
  - pages and components listed above
  - command palette, global search, notifications, guided walkthrough
- Documentation:
  - `docs/WEB_UI.md` (run, security model, troubleshooting)
- Tests:
  - API contracts, UI smoke, streaming reliability

---
End of unified specification.
