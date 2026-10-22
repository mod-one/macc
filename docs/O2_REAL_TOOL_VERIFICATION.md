# Opt-in real tool compatibility verification

`automat/tests/o2_real_tools.py` exercises the existing common runner with reviewed
resolved ToolSpec JSON files. It supports registered adapters without provider-name
branches. Use it for Claude, Codex, Gemini, Vibe and future tools; protocol and
version interpretation remain in each adapter profile.

## Plan first

Provide the resolved `.macc/tool.json` produced for each selected tool by the normal
worktree setup. Keep explicit model/effort values. No model placeholder may remain.
The supplied files are read; the source project is not changed. The default plan
only validates inputs and records executable availability and file hashes. It runs
no tool, probe, package install or model call.

```sh
python3 automat/tests/o2_real_tools.py \
  --tool-json /path/to/codex/tool.json \
  --tool-json /path/to/claude/tool.json \
  --tool-json /path/to/gemini/tool.json \
  --tool-json /path/to/vibe/tool.json \
  --tier standard --report /tmp/o2-tool-plan.json
```

`--tier` is explicit; it must exist in `model_tiers` for tools whose model is
configured through a file. Other tools can retain their already resolved CLI
model/effort by leaving the tier blank. Plans/reports are private, newly created
files; an existing report is never overwritten.

## Execute only on operator request

Repeat the reviewed plan with `--execute --confirm-model-calls` and a **new** report
path. This permits at most five model invocations per selected tool. Calls can be
billed; MACC does not infer a monetary spending limit from a token quota. The
normal `make check` pipeline does not run this target or invoke real models.

```sh
make test-tool-runner-real O2_REAL_ARGS='--tool-json /path/to/tool.json --report /tmp/o2-tool-results.json --execute --confirm-model-calls'
```

Each tool receives a retained disposable Git project with SQLite ownership,
structured task checkpoints, usage capture and a two-invocation rotation boundary.
The harness records source revision, project revision, profile/configuration
hashes, observed CLI version/help/executable identity, session ownership and usage.
Its five calls cover:

1. First performer invocation, with a private conversation nonce.
2. Exact same-task continuation, including nonce recall without reading files.
3. An unrelated task in the same execution slot, with separate history.
4. An independent reviewer for the first task.
5. A fresh performer conversation after checkpoint rotation.

The fixture tracks only its ignore contract. Provider configuration/log files and
local artifacts are disposable and ignored. This is a session compatibility test,
not a representative code-quality or production-cost benchmark.

Invocation timeout defaults to 180 seconds (1–600 allowed), with process-group
termination and no automatic billed retry. Failed/quota/precondition invocations
stop that tool's experiment. Retained leases/worktrees can be inspected explicitly.
Private full invocation logs are stored in its temporary project's `.macc/`.

## Native compression evidence and limitations

`--pressure-bytes` optionally adds up to 65,536 bytes of disposable prompt pressure.
`--native-token-threshold` may configure a supported explicit native token threshold.
Both default to zero. These controls can increase billed input; choose them only
for an intentional experiment with a known model window.

Compression is verified only from the adapter's declared structured compaction
observations. Help output, accepted settings, a textual summary and successful
nonce recall are not compression proof. Shipped profiles without exact observation
paths therefore report **unverified**, preserving safe checkpoint rotation.
Unverified headless slash commands are never attempted. A CLI upgrade changes the
version/executable/profile evidence and requires a new reviewed run; old reports
do not grant a capability to current adapters.

The full four-tool matrix requires four selected profiles and actual execution.
A plan or a fake-tool test never certifies installed model behavior. Non-resumable
or unobservable providers remain explicitly unverified rather than passing by
assumption. Existing deterministic tests cover failure/quota handling, CLI-version
drift, owned-setting changes, mandatory-source preservation and interrupted
recovery without paying for artificial quota exhaustion.

## Next controlled O2 experiment

For short reproducible task pairs and actual usage evidence, start with the
[O2 performance protocol](O2_PERFORMANCE_PROTOCOL.md). Its first experiment
isolates session reuse through registered adapters; it does not claim that the
compatibility nonce fixture measures production savings.

After compatibility checks, use the same pinned tasks, source commits, tool versions,
model/effort and acceptance checks across §23's four arms: safe baseline, reviewed
memory/checkpoints, native map, and managed Graphify. Include interruption/tool
switch, review/correction, test repair and design-fidelity tasks. Capture every
failure/correction and cached usage; manually attest quality from concrete tests.
Use P3-D's supervised pre-registration/adoption gate for policy changes. Report
unknown cost as unknown; fixture timings and token estimates do not prove billed
savings. Keep native compression and representative benefit as separate gates.
