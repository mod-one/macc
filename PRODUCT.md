# Product

## Register

product

## Users

Developers running multiple AI coding agents (Claude Code, Codex, Gemini CLI) on the same codebase. They have configuration scattered across tool-specific files, want parallel worktree execution, and need an orchestration layer they can trust to run unattended.

## Product Purpose

MACC manages canonical config for multiple AI coding tools and coordinates their autonomous execution. It generates tool-specific files from a single source of truth, runs tasks across parallel worktrees, and gives developers a live view of what all their agents are doing. Success means a developer can onboard a new AI tool, start an autonomous run, and go do something else.

## Brand Personality

Calm, orchestrated, reliable. The interface presents information without alarm. Complexity is absorbed by the tool, not exposed to the user. Confidence without arrogance.

## Anti-references

- SaaS marketing README: "Empower your AI workflow," "Supercharge your development," vague benefit stacks.
- Wall-of-text documentation: no hierarchy, no scannability, linear prose.
- Hand-holding onboarding: assumes the reader has never seen a CLI tool. Trust the reader.
- Numbered eyebrow scaffolding (01 / 02 / 03) as default README structure.

## Design Principles

1. **Show the system, not the marketing.** State what MACC does in concrete terms before anything else. The first 10 seconds should answer "is this for me?"
2. **Hierarchy earns its place.** Every heading level, every code block, every list exists because it makes the document faster to scan or understand. No decoration.
3. **Trust the reader.** They know what a CLI is. Skip preambles that explain what a worktree is.
4. **Calm information density.** Dense but not cluttered. Group related commands, leave breathing room between sections.
5. **Commands are the product.** Code blocks are first-class content, not supplementary.

## Accessibility & Inclusion

Standard GitHub Markdown rendering. Heading hierarchy for screen readers and document outlines. Code blocks labeled for syntax highlighting.
