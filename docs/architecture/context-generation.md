# Deterministic native tool context

`core/src/project_context.rs` assembles the native instruction files from the same
adapter asset plan used by Apply. Codex and Vibe share `AGENTS.md`; Gemini and
Antigravity share `GEMINI.md`. Every shared target has one renderer and a sorted
list of consumers. Explicit `context.fileName` targets are validated and honored.
Different contents written to the same path are rejected before Apply starts.

The context contains configured project conventions, references to available
selected skills, and supporting files that exist or are produced by the plan.
It does not manufacture `validate`, `implement`, `explore`, a BMAD command chain,
a package manager, validation commands or a selected model. Per-tool explicit
language overrides project language for that tool's context; incompatible
language overrides for consumers of one shared file are rejected.

A selected skill absent from both the installation and the materialized asset
plan produces a warning, not a synthetic skill or mandatory workflow.

Available selected MACC skills include concise usage guidance:
`macc-performer` for the assigned implementation (or restricted native attempt),
`macc-prd-planner` for PRD creation/revision through the native plan lifecycle,
`macc-reviewer` for independent task review, and `macc-auditor` for final compliance
of the integrated result against requirements and PRD after implementation/review,
before acceptance, merge or release. Auditing reports findings without implementing
fixes. These descriptions do not select all phases at once or override the injected
attempt purpose/decision policy.
Unselected or unavailable skills never acquire a workflow requirement. Third-party
skill contents remain their author's assets; generation does not rewrite them.
Project validation commands remain defined by project/phase requirements. The
supervisor's validation settings are not substituted for performer requirements.

Web standards previews use this assembler, the enabled tools, shared targets and
existing-file protection. Unavailable skill materialization is disclosed rather
than silently represented by a placeholder. `macc context` uses the same native
renderer without an AI call. Its existing flags remain accepted: `--from` adds
references to existing project files; `--print-prompt` prints rendered context;
`--dry-run` previews writes. The applied-project prerequisite remains in force.

With `context.protect: true`, a file created by MACC can migrate if its bytes still
match the generation inventory and it has not been marked user-owned. Updates are
backed up. A preexisting or edited user file is preserved with an explicit warning.
Corrupt ownership evidence is an error, never permission to overwrite. Worktree
synchronization applies the same ownership protection and backs up replacements.
It preserves a worktree's fresh, tool-specific generated bundle instead of copying
in a root bundle that references tools and skills absent from that worktree.

No migration deletes branches, resets task work, removes user files or silently
withdraws an existing decision. An obsolete durable request remains attributed
until native reconciliation establishes why it no longer applies.

Checkpoint recovery preserves unfinished files when the assigned worker changes.
With the coordinator stopped and both workers idle, native recovery may copy
missing untracked regular files into an ancestor-compatible destination. It checks
unchanged task/source contracts on both workers, preflights every destination and
never overwrites, stages, commits or removes the source files. Each copied file
has a session-journal receipt. A conflicting file, tracked edit, live lease,
symlink or changed contract requires explicit reviewed integration; the supervisor
records an infrastructure diagnosis rather than attempting an unrelated source
repair. Recovery compares the retained paths, not whole trees, so a published
prerequisite repair in the destination does not invalidate an otherwise exact copy.
