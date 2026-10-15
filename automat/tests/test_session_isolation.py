#!/usr/bin/env python3
"""Drive the shared runner with a future adapter, without an AI service."""
import copy
import datetime
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
LIBRARY = ROOT / 'adapters/shared/performer_lib.sh'


class SessionIsolation(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.repo = Path(self.tmp.name).resolve()
        (self.repo / '.macc/state').mkdir(parents=True)
        self.tool = self.repo / '.macc/tool.json'
        self.state = self.repo / '.macc/state/tool-sessions.json'
        self.calls = self.repo / 'calls.jsonl'
        self.prompt = self.repo / 'prompt.txt'
        self.prompt.write_text('Implement this task.\n')
        fake = self.repo / 'future-ai'
        fake.write_text(f'''#!{sys.executable}
import json, os, sys, time, uuid
with open(os.environ['CALL_LOG'], 'a') as out:
    out.write(json.dumps(sys.argv[1:]) + '\\n')
if os.environ.get('WAIT_FOR'):
    deadline = time.monotonic() + 10
    while not os.path.exists(os.environ['WAIT_FOR']) and time.monotonic() < deadline:
        time.sleep(0.02)
args = sys.argv[1:]
sid = args[1] if args[0] in ['create', 'resume'] else str(uuid.uuid4())
print('session id: ' + sid)
if os.environ.get('CONFLICTING_ID'):
    print('session id: foreign-session')
print('MACC_TASK_RESULT: success_without_changes')
''')
        fake.chmod(0o755)
        self.spec = {'id': 'future-ai', 'performer': {
            'command': str(fake), 'args': ['new'], 'prompt': {'mode': 'arg'},
            'retry': {'command': str(fake), 'args': ['resume', '{session_id}']},
            'session': {'enabled': True, 'scope': 'task_role_tool', 'id_strategy': 'generated',
                        'create': {'command': str(fake), 'args': ['create', '{session_id}']},
                        'resume': {'command': str(fake), 'args': ['resume', '{session_id}']}}}}
        self.save_spec()
        self.env = dict(os.environ, CALL_LOG=str(self.calls), MACC_SESSION_ROLE='performer',
                        MACC_MODEL_TIER='', MACC_MODEL_ROUTING_MODE='auto')

    def save_spec(self):
        self.tool.write_text(json.dumps(self.spec))

    def identity(self, task='A', role='performer'):
        return {'version': 1, 'project_root': str(self.repo), 'execution_root': str(self.repo),
                'task_id': task, 'role': role, 'tool_id': 'future-ai', 'scope': 'task_role_tool',
                'execution_config': {'performer': self.spec['performer'],
                                     'model_tiers': self.spec.get('model_tiers'),
                                     'tier': '', 'routing': 'auto'}}

    def seed(self, identity=None, **changes):
        now = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
        entry = {'session_identity': identity or self.identity(), 'status': 'available',
                 'updated_at': now, 'created_at': now, 'use_count': 1}
        entry.update(changes)
        self.state.write_text(json.dumps({'tools': {'future-ai': {'sessions': {'seeded-session': entry}}}}))

    def argv(self, task='A', hint=None, attempt=1):
        args = ['bash', '-c', 'TOOL_ID=future-ai; TOOL_LOG_PREFIX=test; source "$1" "${@:2}"',
                '--', str(LIBRARY), '--prompt-file', str(self.prompt), '--tool-json', str(self.tool),
                '--repo', str(self.repo), '--worktree', str(self.repo), '--task-id', task,
                '--attempt', str(attempt)]
        return args + (['--session-id', hint] if hint else [])

    def run_tool(self, task='A', hint=None, attempt=1, **env):
        result = subprocess.run(self.argv(task, hint, attempt), cwd=self.repo,
                                env=dict(self.env, **env), capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return json.loads(self.calls.read_text().splitlines()[-1])

    def entries(self):
        return json.loads(self.state.read_text())['tools']['future-ai']['sessions']

    def test_same_task_resumes_and_release_keeps_semantic_identity(self):
        self.seed()
        self.assertEqual(self.run_tool()[:2], ['resume', 'seeded-session'])
        entry = self.entries()['seeded-session']
        self.assertEqual(entry['status'], 'available')
        self.assertEqual(entry['session_identity'], self.identity())
        self.assertNotIn('owner_pid', entry)
        events = [json.loads(line) for line in (self.repo / '.macc/log/events.jsonl').read_text().splitlines()]
        reused = next(event for event in events if event['type'] == 'session_reused')
        self.assertEqual(reused['payload']['role'], 'performer')
        self.assertIn('role=performer', reused['payload']['message'])

    def test_different_task_rejects_both_pool_and_caller_hint(self):
        self.seed()
        for hint in [None, 'seeded-session', 'unknown-provider-id']:
            with self.subTest(hint=hint):
                call = self.run_tool(task='B', hint=hint)
                self.assertNotIn('seeded-session', call)
                self.assertNotIn('unknown-provider-id', call)
        self.assertEqual(self.entries()['seeded-session']['session_identity']['task_id'], 'A')

    def test_reviewer_isolation_and_performer_continuation(self):
        self.seed()
        review = self.run_tool(hint='seeded-session', MACC_SESSION_ROLE='reviewer')
        self.assertEqual(review[0], 'create')
        self.assertEqual(self.run_tool()[:2], ['resume', 'seeded-session'])

    def test_legacy_closed_and_incompatible_identity_are_not_adopted(self):
        for mutation in ['legacy', 'closed', 'model', 'project', 'environment', 'tool']:
            self.seed()
            state = json.loads(self.state.read_text())
            entry = state['tools']['future-ai']['sessions']['seeded-session']
            if mutation == 'legacy':
                del entry['session_identity']
                entry['last_task_id'] = 'A'
            elif mutation == 'closed': entry['status'] = 'closed'
            elif mutation == 'model': entry['session_identity']['execution_config']['tier'] = 'different'
            elif mutation == 'project': entry['session_identity']['project_root'] = '/another/project'
            elif mutation == 'environment': entry['session_identity']['execution_root'] = '/another/worktree'
            else: entry['session_identity']['tool_id'] = 'another-tool'
            self.state.write_text(json.dumps(state))
            with self.subTest(mutation=mutation):
                self.assertNotIn('seeded-session', self.run_tool(hint='seeded-session'))

    def test_live_or_ambiguous_owner_never_evicted_by_expired_heartbeat(self):
        for owner in [str(os.getpid()), None, '0', '-1', '99999999']:
            self.seed(status='active', owner_pid=owner, heartbeat_epoch=0)
            with self.subTest(owner=owner):
                self.assertNotIn('seeded-session', self.run_tool(hint='seeded-session'))
                self.assertEqual(self.entries()['seeded-session']['status'], 'active')

    def test_expired_explicit_hint_is_rejected(self):
        self.seed(last_used_at='2000-01-01T00:00:00Z')
        self.assertNotIn('seeded-session', self.run_tool(hint='seeded-session'))

    def test_generated_strategy_without_create_starts_fresh_even_on_retry(self):
        del self.spec['performer']['session']['create']
        self.save_spec()
        call = self.run_tool(attempt=2)
        self.assertEqual(call[0], 'new')
        self.assertNotIn('resume', call)

    def test_fresh_generated_retry_does_not_merge_resume_flags(self):
        call = self.run_tool(attempt=2)
        self.assertEqual(call[0], 'create')
        self.assertNotIn('resume', call)

    def test_global_discovery_never_adopts_an_unrelated_id(self):
        session = self.spec['performer']['session']
        session['id_strategy'] = 'discovered'
        session.pop('create')
        marker = self.repo / 'discovery-was-called'
        session['discover'] = {'command': 'bash', 'args': ['-c', f'touch {marker}; echo foreign-session']}
        self.save_spec()
        self.assertEqual(self.run_tool()[0], 'new')
        self.assertFalse(marker.exists())
        self.assertNotIn('foreign-session', self.entries())

    def test_conflicting_metadata_does_not_adopt_a_provider_id(self):
        self.spec['performer']['session']['id_strategy'] = 'discovered'
        del self.spec['performer']['session']['create']
        self.save_spec()
        self.assertEqual(self.run_tool(CONFLICTING_ID='yes')[0], 'new')
        self.assertFalse(self.state.exists())

    def test_corrupt_store_is_preserved_and_no_tool_is_invoked(self):
        self.state.write_text('{broken json')
        result = subprocess.run(self.argv(), cwd=self.repo, env=self.env,
                                capture_output=True, text=True, timeout=20)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('invalid session store', result.stderr)
        self.assertEqual(self.state.read_text(), '{broken json')
        self.assertFalse(self.calls.exists())

    def test_generated_id_collision_cannot_recreate_an_existing_transcript(self):
        self.seed()
        args = self.argv(task='B')
        args[2] = 'uuidgen() { echo seeded-session; }; ' + args[2]
        result = subprocess.run(args, cwd=self.repo, env=self.env,
                                capture_output=True, text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        call = json.loads(self.calls.read_text().splitlines()[-1])
        self.assertEqual(call[0], 'new')
        self.assertNotIn('seeded-session', call)
        self.assertEqual(self.entries()['seeded-session']['session_identity']['task_id'], 'A')

    def test_no_task_identity_disables_pool(self):
        self.seed()
        before = self.state.read_bytes()
        self.assertEqual(self.run_tool(task='', hint='seeded-session')[0], 'new')
        self.assertEqual(before, self.state.read_bytes())

    def test_missing_flock_runs_fresh_without_mutating_pool(self):
        self.seed()
        before = self.state.read_bytes()
        args = self.argv(hint='seeded-session')
        args[2] = ('command() { if [[ "$1" == -v && "$2" == flock ]]; then return 1; fi; '
                   'builtin command "$@"; }; ' + args[2])
        result = subprocess.run(args, cwd=self.repo, env=self.env, capture_output=True,
                                text=True, timeout=20)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('session reuse disabled', result.stderr)
        self.assertEqual(json.loads(self.calls.read_text().splitlines()[-1])[0], 'new')
        self.assertEqual(before, self.state.read_bytes())

    def test_concurrent_claim_never_resumes_the_same_session(self):
        self.seed()
        release = self.repo / 'release'
        first = subprocess.Popen(self.argv(hint='seeded-session'), cwd=self.repo,
                                 env=dict(self.env, WAIT_FOR=str(release)),
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        self.addCleanup(lambda: first.poll() is None and first.kill())
        import time
        deadline = time.monotonic() + 5
        while not self.calls.exists() and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertTrue(self.calls.exists(), 'first performer did not start')
        self.assertNotIn('seeded-session', self.run_tool(hint='seeded-session'))
        release.touch()
        out, err = first.communicate(timeout=10)
        self.assertEqual(first.returncode, 0, out + err)
        self.assertEqual(self.entries()['seeded-session']['status'], 'available')

    def test_same_branch_dirty_work_is_preserved(self):
        subprocess.run(['git', 'init', '-q', str(self.repo)], check=True)
        tracked = self.repo / 'implementation.txt'
        tracked.write_text('initial\n')
        subprocess.run(['git', '-C', str(self.repo), 'add', 'implementation.txt'], check=True)
        subprocess.run(['git', '-C', str(self.repo), '-c', 'user.name=Test', '-c',
                        'user.email=test@example.invalid', 'commit', '-qm', 'initial'], check=True)
        branch = subprocess.check_output(['git', '-C', str(self.repo), 'symbolic-ref', 'HEAD'])
        tracked.write_text('staged work\n')
        subprocess.run(['git', '-C', str(self.repo), 'add', 'implementation.txt'], check=True)
        tracked.write_text('staged and unstaged work\n')
        untracked = self.repo / 'new-module.txt'
        untracked.write_text('unfinished implementation\n')
        index_before = subprocess.check_output(['git', '-C', str(self.repo), 'show', ':implementation.txt'])
        self.seed()
        self.run_tool(hint='seeded-session', MACC_SESSION_ROLE='reviewer')
        self.assertEqual(tracked.read_text(), 'staged and unstaged work\n')
        self.assertEqual(untracked.read_text(), 'unfinished implementation\n')
        self.assertEqual(index_before, subprocess.check_output(['git', '-C', str(self.repo), 'show', ':implementation.txt']))
        self.assertEqual(branch, subprocess.check_output(['git', '-C', str(self.repo), 'symbolic-ref', 'HEAD']))


if __name__ == '__main__':
    unittest.main(verbosity=2)
