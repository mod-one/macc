# Reproducible skill sources and reviewed installations

## Decision

Resolve selected Git skills from `.macc/skills.lock.json`, materialize content into immutable shared snapshots and copy bytes into each project/worktree. Git commits are pinned automatically; HTTP archives use SHA-256. Normal application never advances an existing pin. An explicit update resolves the saved tracking ref again. Availability checks retain their separate expiry/cache policy.

Publish snapshots atomically under per-source advisory locks and verify a receipt containing source identity and tree fingerprint on reuse. Regular files are read-only; snapshots contain no mutable Git checkout metadata. Preserve previous versions, migrate old cached Git objects without editing old checkouts, and read a reference project's fallback cache from workers. Bind lockfile mutations to the revision reviewed by the client; reject local changes whose recorded digest differs.

## Ports and client boundaries

`MaterializedFetchUnit` carries optional source provenance and selections carry optional tool scopes. Old serialized units remain readable. Shared installation planning uses adapter-declared skill roots, so native layouts such as AGY's `.agents/skills` stay behind the adapter interface. One narrow `Engine::skill_install_directory` port intentionally changes the Engine surface guard from 146 to 147 methods; the default preserves the existing convention for custom adapters.

The two CLI installation spellings delegate to the same skill lifecycle command. It reviews exact versions/files and applies only package files plus the lock receipt. It does not perform unrelated tool account setup. Full `macc apply` retains its complete project/tool/O2 preparation boundary and uses the same file/lock planning services. Cancellation restores the staged tool selection through a configuration revision check; failures during application retain recoverable partial work.

## Migration and preservation

Selected legacy intent locks are completed by confirmed Apply. Existing worker pins survive retries; newly created workers inherit project pins. Clear retains pins, configuration saves capture them, and restores validate them before overwriting configuration. Caches stay outside save bundles. Existing version-0 lockfiles are accepted as version 1; unsupported future versions fail. Explicit local source directories retain their live development behavior.

## Tradeoffs

Tree hashing adds local I/O on reuse in exchange for corruption detection. Different selected path sets can require separate snapshots; cached supersets can serve narrower Git requests. An expired availability check or project readiness check can still contact its service independently of source reuse. Snapshots are intentionally retained without automatic garbage collection of potentially referenced versions.
