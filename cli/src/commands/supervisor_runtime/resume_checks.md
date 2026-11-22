For each observed root prerequisite, propose a recovery dossier in the optional
top-level resume_checks array. Classify by what releases the task. Use
technical_condition for an independently observable prerequisite, external_action
for access or work that cannot yet be observed, human_approval for required
votes, and review_correction for rejected work. The latter three have no verifier.
Keep ordinary source defects on the repair path. Name unrelated repairable roots
separately. Never replace device measurements, review, authorization or acceptance
with an observation that only establishes readiness to run them.

Each dossier has task_id, phase (the retained dev/test/review/fix/merge),
category, target (exact stable identifier), criterion (verbatim expected_result
of a native intervention, PRD blocked_on_external.clears_when, or sole unmet
PRD criterion), decision (exact {id,version} for a native wait, otherwise null)
and verifier. For technical_condition provide the complete verifier object
{source,script}. Use only a target supported by the task and runner evidence.
If the serial, report provenance, authority or other essential input is
unknown, report the missing action. Never substitute any connected device or
an unrelated build for the intended candidate.

Write the script as a single read-only SQLite SELECT with one parameter :input.
It receives JSON with target, commit (candidate HEAD), now_unix and observation.
Choose a runner observation source: {"kind":"adb_devices"} supplies an array of
{serial,state,attributes}; {"kind":"json_file","path":"..."} or
{"kind":"text_file","path":"..."} supplies an artifact relative to the retained
worktree. Use existing runner evidence and actual report fields. A missing file
waits automatically. Other sources of proof may supply a local report.
Return exactly one row/column containing a JSON object with status
ready, waiting or error and an actionable nonempty detail (up to 1024 bytes).
Check the exact target and every part of the prerequisite. For CI include the
candidate commit, environment, result and timestamp window. For ADB require
the reserved serial and authorized device state. Only MACC executes this script
at coordinator startup and decides whether the original phase can resume.
