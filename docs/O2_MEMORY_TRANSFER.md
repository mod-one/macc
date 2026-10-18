# Reviewed memory import and export

The CLI, TUI and Web call the same operator-only application actions. Transfer
supports canonical JSONL and MACC Markdown (a readable envelope containing the
same JSONL rows). It is separate from full-project `macc save`/restore.

## Workflow

1. Select 1–100 explicit memory IDs and matching task/tool/feature/worktree/user
   access. Project memories are in scope. Choose JSONL or Markdown.
2. Preview the exact records. Source checksums, schema, scope, secret/control
   checks and the current configuration revision bind the confirmation checksum.
3. Explicitly confirm the same request/checksum. An edited record, changed review
   or changed configuration requires another preview.
4. Export the document or import it as **proposals**. Imported source status,
   reviewer identity and commit provenance never grant local active status.
5. Inspect each proposed fact and recapture/revalidate evidence before activation.

Imports are atomic, do not expand scopes and never overwrite an existing local
review. Source project/record/status/checksum are recorded in audit events.
Duplicate transfers resolve to deterministic imported IDs; renewed preview/replay
preserves later local decisions. Unknown schema fields, malformed rows, duplicate
rows, secrets, invalid scopes/checksums and transfers above 2 MiB are rejected.
The fixed 100-record/2-MiB bounds are safety invariants, not editable settings.
Markdown import accepts the exact MACC envelope, not arbitrary Markdown documents.

Previews use a read-only database connection and perform no installation, model
call, activation or import/export audit write. Apply uses the normal operator
ownership gate, authority lock and SQLite transaction. Agent MCP exposes none of
these transfer actions. An operator with unrestricted shell access is outside the
MCP privilege boundary.

## Clients

- **Web → Memory → Reviewed memory transfer:** choose import/export and format;
  enter IDs or use the selected memory; choose a local file or paste a document;
  preview, check explicit confirmation and Apply. Export downloads the confirmed
  document. Changing records, scope or format clears the prior confirmation.
- **TUI → Memory:** `E` opens guided export (IDs, format, new output path); `I`
  opens guided import (input path, format). Inspect the preview, then `a` applies
  or `D` discards. Relative paths resolve from the project. Export creates a new
  private file and refuses existing paths; it never overwrites a file.
- **CLI:** the four commands below accept a typed JSON request on stdin and return
  the shared JSON envelope. `--confirm` is required for Apply.

```text
macc o2 memory-export-preview
macc o2 memory-export CHECKSUM --confirm
macc o2 memory-import-preview
macc o2 memory-import CHECKSUM --confirm
```

Export request:

```json
{"ids":["MEMORY_ID"],"format":"jsonl","access":{"task_id":"TASK-ID"}}
```

Import request:

```json
{"document":"EXACT_EXPORTED_DOCUMENT","format":"jsonl","access":{"task_id":"TASK-ID"}}
```

The exported text is the `document` field of the export response. Build the import
request with a JSON encoder so document newlines and quotes are preserved. The
generic `macc o2 action` transport also supports `memory_export_preview`,
`memory_export`, `memory_import_preview` and `memory_import`.

No SQLite schema migration is needed: imported proposals, FTS projection and audit
events use the existing canonical tables, so existing save/restore includes them.
