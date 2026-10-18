#!/usr/bin/env python3
"""Opt-in real AI compatibility checks through MACC's existing ToolSpec/runner seam.

Default mode validates a plan only: no CLI probes or model calls. Every execution
uses a retained disposable Git project and writes a versioned evidence report.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import sqlite3
import subprocess
import tempfile
import time
import uuid

SOURCE = Path(__file__).resolve().parents[2]


def digest(value):
    return hashlib.sha256(value).hexdigest()


def run(args, cwd, env, timeout=30, input_text=None):
    """Kill the whole invocation group on timeout; never retry a billed call."""
    with tempfile.TemporaryFile() as output:
        process = subprocess.Popen(args, cwd=cwd, env=env, stdin=subprocess.PIPE,
                                   stdout=output, stderr=subprocess.STDOUT,
                                   start_new_session=True)
        try:
            process.communicate(input_text.encode() if input_text else None, timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=3)
            raise RuntimeError("Invocation timed out; retained work/lease needs inspection")
        size = output.tell()
        output.seek(0)
        text = output.read(2 * 1024 * 1024 + 1).decode(errors='replace')
        if size > 2 * 1024 * 1024:
            raise RuntimeError("Invocation output exceeded evidence bound")
        if process.returncode:
            # Raw output stays private; errors in the public report contain no credentials.
            raise RuntimeError(f"Invocation exited {process.returncode}; inspect private fixture logs")
        return text


def profile(path):
    raw = path.read_bytes()
    if len(raw) > 1024 * 1024:
        raise ValueError('Resolved ToolSpec exceeds 1 MiB')
    spec = json.loads(raw)
    tool = spec['id']
    if not re.fullmatch(r'[a-z][a-z0-9-]{0,63}', tool):
        raise ValueError('Invalid ToolSpec ID')
    p = spec['performer']
    # Provider session placeholders are resolved by the runner; model/effort must
    # already reflect the actual reviewed project configuration.
    if re.search(r'\{(?:model|effort)\}', json.dumps(p)):
        raise ValueError('Use a resolved tool.json with explicit model and effort')
    return spec, digest(raw)


def exercise(spec, checksum, args):
    root = Path(tempfile.mkdtemp(prefix=f'macc-o2-real-{spec["id"]}-'))
    env = dict(os.environ, MACC_INTERNAL_INVOCATION='1', MACC_SESSION_SERVICE_BIN=str(args.macc))
    macc = [str(args.macc), '--cwd', str(root)]
    report = {'tool': spec['id'], 'spec_sha256': checksum, 'fixture': str(root),
              'source_revision': args.source_revision, 'calls': 0, 'checks': {},
              'started_at': int(time.time()), 'tier': args.tier, 'status': 'failed'}
    try:
        def git(*values):
            return run(['git', *values], root, env)
        git('init', '-b', 'main')
        git('config', 'user.email', 'smoke@example.invalid')
        git('config', 'user.name', 'MACC smoke')
        (root / '.gitignore').write_text('*\n!.gitignore\n')
        git('add', '.gitignore')
        git('commit', '-m', 'smoke baseline')
        (root / '.macc/tools.d').mkdir(parents=True)
        (root / '.macc/tool.json').write_text(json.dumps(spec))
        # JSON is valid YAML; retain complete ToolSpec for version/profile probes.
        registered = dict(spec, api_version=spec.get('api_version', 'v1'), display_name=spec.get('display_name', spec['id']), fields=[])
        (root / f'.macc/tools.d/{spec["id"]}.tool.yaml').write_text(json.dumps(registered))
        config = ('tools:\n  enabled: []\nsession_management:\n  checkpoints:\n'
                  '    rotate_after_uses: 2\n  usage:\n    capture_mode: 1\n'
                  'adaptive_compaction:\n  enabled: true\n  configure_native_auto: true\n'
                  f'  native_token_threshold: {args.native_token_threshold}\n')
        (root / '.macc/macc.yaml').write_text(config)
        (root / 'worktree.prd.json').write_text(json.dumps({'tasks': [
            {'id': t, 'acceptance_criteria': ['Do not modify tracked implementation files',
                                             'Follow the checkpoint acknowledgement contract']}
            for t in ['SMOKE-A', 'SMOKE-B']]}))
        plan = json.loads(run(macc + ['session-store', 'migrate'], root, env))
        run(macc + ['session-store', 'migrate', '--apply', '--expected-sha256', plan['source_sha256']], root, env)
        probe = json.loads(run(macc + ['o2', 'compaction-probe', spec['id']], root, env))
        report['probe'] = probe
        report['config_sha256'] = digest(config.encode())
        report['project_revision'] = git('rev-parse', 'HEAD').strip()
        report['profile'] = spec['performer'].get('compaction')
        script = SOURCE / 'adapters/shared/performer_lib.sh'
        secret = uuid.uuid4().hex
        snapshots = []
        prompts = [
            ('SMOKE-A', 'performer', f'Remember private token {secret} only in this conversation. Do not write it to files.'),
            ('SMOKE-A', 'performer', 'Return the private token from the preceding invocation. Do not inspect files or logs.'),
            ('SMOKE-B', 'performer', 'If a private token is known, return it; otherwise say UNKNOWN. Do not inspect files or logs.'),
            ('SMOKE-A', 'reviewer', 'If a private token is known, return it; otherwise say UNKNOWN. Do not inspect files or logs.'),
            ('SMOKE-A', 'performer', 'A rotation was requested. If a private token is known return it, otherwise UNKNOWN. Do not inspect files or logs.'),
        ]
        for index, (task, role, prompt) in enumerate(prompts):
            prompt += '\nDo not change repository files. Emit the requested checkpoint ACK and finish with MACC_TASK_RESULT: success_without_changes\n'
            if index < 2 and args.pressure_bytes:
                prompt += '\nDisposable pressure fixture:\n' + ('0123456789 abcdefghijklmnopqrstuvwxyz\n' * (args.pressure_bytes // 37))
            prompt_file = root / '.macc/prompt.txt'
            prompt_file.write_text(prompt)
            call_env = dict(env, TOOL_ID=spec['id'], TOOL_LOG_PREFIX=spec['id'],
                            MACC_MODEL_TIER=args.tier, MACC_MODEL_ROUTING_MODE='auto',
                            MACC_SESSION_ROLE=role, MACC_SESSION_PHASE='review' if role == 'reviewer' else 'dev')
            report['calls'] += 1
            output = run(['bash', '-c', 'source "$1" "${@:2}"', '--', str(script),
                          '--repo', str(root), '--worktree', str(root), '--task-id', task,
                          '--tool-json', str(root / '.macc/tool.json'), '--prompt-file', str(prompt_file)],
                         root, call_env, args.timeout)
            log = root / f'.macc/invocation-{index}.txt'
            log.write_text(output)
            log.chmod(0o600)
            verdicts = re.findall(r'^MACC_TASK_RESULT:\s*(\S+)\s*$', output, re.MULTILINE)
            if not verdicts or verdicts[-1] != 'success_without_changes':
                raise RuntimeError('Provider did not complete the smoke contract; inspect retained invocation log; no retry')
            view = json.loads(run(macc + ['session-store', 'status'], root, env))
            current = [s for s in view['sessions'] if s['task_id'] == task and s['role'] == role]
            current.sort(key=lambda s: (s['updated_at'], s['use_count']), reverse=True)
            snapshots.append((current, output))
        first, second = snapshots[0][0][0], snapshots[1][0][0]
        known = first['provider_session_id'] is not None
        checks = report['checks']
        checks['exact_resume'] = 'verified' if known and first['id'] == second['id'] and first['provider_session_id'] == second['provider_session_id'] and secret in snapshots[1][1] else 'unverified'
        # Session store is the authority for ownership; nonce checks additionally
        # detect unexpected chat leakage in provider responses.
        other, reviewer = snapshots[2][0][0], snapshots[3][0][0]
        checks['task_isolation'] = 'verified' if other['id'] != first['id'] and secret not in snapshots[2][1] else 'failed'
        checks['reviewer_isolation'] = 'verified' if reviewer['id'] != first['id'] and secret not in snapshots[3][1] else 'failed'
        fresh = [s for s in snapshots[4][0] if s['id'] != first['id']]
        checks['checkpoint_rotation'] = 'verified' if fresh and secret not in snapshots[4][1] else 'failed'
        with sqlite3.connect(f'file:{root}/.macc/memory.db?mode=ro', uri=True) as db:
            counts = [r[0] for r in db.execute('SELECT compactions FROM compaction_pressure WHERE compactions IS NOT NULL')]
        checks['native_compression'] = 'verified' if counts and max(counts) > 0 else 'unverified'
        checks['explicit_headless'] = 'unsupported: no unverified slash command executed'
        report['sessions'] = view['sessions']
        report['usage'] = json.loads(run(macc + ['session-store', 'usage'], root, env))
        report['status'] = 'failed' if 'failed' in checks.values() else 'measured'
    except (OSError, ValueError, RuntimeError, KeyError) as error:
        report['error'] = str(error)[:500]
    report['finished_at'] = int(time.time())
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tool-json', type=Path, action='append', required=True,
                        help='Resolved project tool.json; repeat for Claude/Codex/Gemini/Vibe')
    parser.add_argument('--macc', type=Path, default=SOURCE / 'target/debug/macc')
    parser.add_argument('--report', type=Path, required=True, help='New evidence file; never overwritten')
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--confirm-model-calls', action='store_true')
    parser.add_argument('--tier', default='', help='Explicit model tier, required for config-file model adapters')
    parser.add_argument('--timeout', type=int, default=180)
    parser.add_argument('--pressure-bytes', type=int, default=0)
    parser.add_argument('--native-token-threshold', type=int, default=0)
    args = parser.parse_args(argv)
    if args.execute and not args.confirm_model_calls:
        parser.error('--execute requires --confirm-model-calls (at most five model invocations per tool)')
    if not 1 <= args.timeout <= 600 or not 0 <= args.pressure_bytes <= 65536 or not 0 <= args.native_token_threshold <= 16000000:
        parser.error('Timeout/pressure/native threshold outside supported bounds')
    args.macc = args.macc.resolve()
    specs = [profile(path) for path in args.tool_json]
    for spec, _ in specs:
        if spec['performer'].get('model_config') and args.tier not in spec.get('model_tiers', {}):
            parser.error('Config-file model adapter requires an explicit --tier present in model_tiers')
    if len({spec['id'] for spec, _ in specs}) != len(specs):
        parser.error('Select each tool once')
    args.source_revision = subprocess.check_output(['git', '-C', str(SOURCE), 'rev-parse', 'HEAD'], text=True).strip()
    report = {'schema_version': 1, 'mode': 'execute' if args.execute else 'plan',
              'maximum_model_calls': 5 * len(specs), 'tools': []}
    # Reserve private output before any probe or model invocation.
    with args.report.open('x', encoding='utf-8') as output:
        args.report.chmod(0o600)
        for spec, checksum in specs:
            report['tools'].append(exercise(spec, checksum, args) if args.execute else {
                'tool': spec['id'], 'spec_sha256': checksum, 'command': spec['performer']['command'],
                'installed': shutil.which(spec['performer']['command']) is not None,
                'status': 'planned; no probe or model call executed'})
        json.dump(report, output, indent=2)
        output.write('\n')
    return int(any(t.get('status') == 'failed' for t in report['tools']))


if __name__ == '__main__':
    raise SystemExit(main())
