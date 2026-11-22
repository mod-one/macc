# Resuming a blocked task

Once the supervisor has recorded a technical check, restore the missing condition and run `macc coordinator run` in the project. In the TUI, use **Operate → Coordinator live → Run coordinator**. Select a blocked row to see its condition, retained phase and most recent result. A missing phone, an unauthorized transport and a failure to contact the runner's ADB server can require different operator actions.

| What the task is waiting for | What releases it |
| --- | --- |
| Technical condition | A current, successful observation of the recorded prerequisite |
| External action | The named access or action. A subsequent diagnosis can bind a check when the result becomes observable |
| Human approval | The existing revision, evidence, roles and quorum rules |
| Review correction | A correction through the retained review workflow |

A successful readiness check resumes the task's work. For example, Android measurements still execute in the retained worker after the reserved device is reachable. The coordinator records an observed intervention as machine evidence. The task still goes through its normal validation and publication rules.

## Supervisor dossier

The supervisor can include a `resume_checks` array in its diagnosis. Each item names the task, phase (`dev`, `test`, `review`, `fix` or `merge`), category, target and exact criterion. A native intervention also requires its decision ID and version. A technical item includes a verifier with `source` and `script`. Other categories omit the verifier. The supervisor generates each script for the incident without requiring the operator to register a command.

MACC stores the complete dossier under `.macc/state/resume-checks/`. The registry references its SHA-256 ID. The dossier binds the script digest, PRD contract, error and attempt, candidate commit, worker assignment and decision version when applicable. An altered dossier, decision, phase or candidate leaves the task blocked. Checks run once at startup for the active PRD, before dispatch. Opening the TUI or refreshing status reads the recorded result.

## Writing a check

The script is a read-only SQLite query. It receives a single named parameter, `:input`, containing a JSON object with `observation`, `target`, `commit` and `now_unix`. It must return exactly one cell containing a JSON object such as `{"status":"waiting","detail":"Unlock the reserved phone and authorize the runner"}`. Valid statuses are `ready`, `waiting` and `error`. Write the detail for the operator and omit secrets.

| Source | Observation supplied to the script |
| --- | --- |
| `{"kind":"adb_devices"}` | Inventory from the local ADB server on port 5037, including serial, transport state and reported attributes |
| `{"kind":"json_file","path":"reports/ci.json"}` | Parsed JSON from the retained worktree, or from the project for an unassigned task |
| `{"kind":"text_file","path":"reports/runner.txt"}` | UTF-8 text from a runner artifact, using the same path rules |

Project artifact paths are relative and cannot traverse symlinks or hidden directories. CI should deliver its report to the retained worktree. The check can validate fields such as candidate SHA, runner identity, environment and age. The report's producer is responsible for its authenticity. An absent file yields a wait. An unreadable file, invalid result, unavailable ADB server or exhausted execution budget yields a verifier error. The runner bounds observation size, SQL instructions, result size and execution time.

The examples are executed by the regression tests. The [device check](adb.sql) matches the reserved serial and requires an authorized transport. The [CI check](ci.sql) requires a passing result for the candidate commit and target environment, finished within the last hour.
