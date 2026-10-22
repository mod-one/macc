#!/usr/bin/env python3
"""Deterministic tests; never launch a real AI CLI."""
import contextlib
import copy
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from benchmarks.o2 import cli, fixtures, metrics, retention, navigation_experiment
from benchmarks.o2.transport import Commands
from benchmarks.o2.transport import ExperimentInterrupted


class O2BenchmarkTests(unittest.TestCase):
    def test_plan_has_no_model_call_or_probe_and_never_overwrites(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            settings = root / 'settings.json'
            settings.write_text(json.dumps({tool: {'model_tiers': {'standard': {'model': 'fixture', 'effort': 'low'}}} for tool in ['codex', 'claude']}))
            argv = ['--settings', str(settings), '--output', str(root / 'plan')]
            with patch.object(cli, 'exercise', side_effect=AssertionError('No model calls in plan')), patch.object(Commands, 'run', side_effect=AssertionError('No probes in plan')):
                with contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(cli.main(argv), 0)
            report = json.loads((root / 'plan/report.json').read_text())
            self.assertEqual(report['maximum_model_calls'], 8)
            self.assertEqual(report['arms'], [])
            self.assertEqual((root / 'plan').stat().st_mode & 0o777, 0o700)
            with self.assertRaises(FileExistsError):
                cli.main(argv)

    def test_known_sums_never_make_missing_dimensions_exact(self):
        records = [{'incremental': {'input_uncached': 0, 'output': 10}, 'elapsed_ms': None}, {'incremental': {'output': 5}, 'elapsed_ms': 2000}]
        result = metrics.measured(records)
        self.assertEqual(result['known_sums']['input_uncached'], 0)
        self.assertEqual(result['coverage']['input_uncached'], {'known': 1, 'expected': 2})
        self.assertIsNone(result['known_sums']['cached_read'])
        self.assertIsNone(result['invocation_elapsed_ms'])

    def test_quality_profile_coverage_and_cost_provenance_gate_comparisons(self):
        records = [{'incremental': {'input_uncached': 100, 'cost': 20}, 'model': 'fixed', 'provider_version': '1.0', 'cost_quality': 'estimated', 'currency': 'USD', 'status': 'recorded', 'outcome': 'success'}] * 2
        base = {'tool': 'fixture', 'case': 'slug-list', 'arm': 'fresh', 'accepted': True,
                'session_behavior_verified': True, 'metrics': metrics.measured(records),
                'starting_revision': 'a', 'config_sha256': 'b', 'profile_sha256': 'c'}
        candidate = copy.deepcopy(base)
        candidate['arm'] = 'reuse'
        candidate['metrics']['known_sums']['input_uncached'] = 100
        pair = metrics.compare([base, candidate])[0]
        self.assertTrue(pair['eligible'])
        self.assertEqual(pair['reduction_percent']['input_uncached'], 50)
        self.assertIsNone(pair['reduction_percent']['cost'])
        candidate['metrics']['coverage']['input_uncached']['known'] = 1
        self.assertIsNone(metrics.compare([base, candidate])[0]['reduction_percent']['input_uncached'])
        candidate['metrics']['coverage']['input_uncached']['known'] = 2
        candidate['profile_sha256'] = 'changed'
        self.assertFalse(metrics.compare([base, candidate])[0]['eligible'])
        candidate['profile_sha256'] = 'c'
        candidate['metrics']['complete_successful_receipts'] = False
        self.assertFalse(metrics.compare([base, candidate])[0]['eligible'])
        candidate['metrics']['complete_successful_receipts'] = True
        candidate['accepted'] = False
        self.assertFalse(metrics.compare([base, candidate])[0]['eligible'])

    def test_independent_checks_catch_wrong_behavior_and_input_mutation(self):
        with tempfile.TemporaryDirectory() as name:
            path = Path(name) / 'solution.py'
            path.write_text('def slug(text): return "-".join(text.lower().split())\ndef unique_slugs(values): return list(dict.fromkeys(filter(None, map(slug, values))))\n')
            self.assertEqual(fixtures.verify(path, 'slug-list', 1), 5)
            path.write_text('def slug(text): return "wrong"\n')
            with self.assertRaises(AssertionError):
                fixtures.verify(path, 'slug-list', 0)
            path.write_text('def slug(text): return "-".join(text.lower().split())\ndef unique_slugs(values):\n result = list(dict.fromkeys(filter(None, map(slug, values))))\n values.append("mutated")\n return result\n')
            with self.assertRaisesRegex(AssertionError, 'mutated'):
                fixtures.verify(path, 'slug-list', 1)
            path.write_text('def clamp(value, lower, upper):\n if lower > upper: return {"raises": "ValueError"}\n return min(upper, max(lower, value))\n')
            with self.assertRaisesRegex(AssertionError, 'expected ValueError'):
                fixtures.verify(path, 'bounded-values', 0)

    def test_timeout_and_failed_commands_are_retained_without_retry(self):
        with tempfile.TemporaryDirectory() as name:
            commands = Commands(name)
            with self.assertRaisesRegex(RuntimeError, 'timed out'):
                commands.run([sys.executable, '-c', 'import time; time.sleep(30)'], name, timeout=0.02)
            entries = json.loads((Path(name) / 'commands.json').read_text())
            self.assertEqual(len(entries), 1)
            self.assertEqual(entries[0]['status'], 'timeout')
            self.assertTrue(Path(entries[0]['log']).exists())

    def test_failed_tool_is_not_retried_on_another_case(self):
        self._failed_tool_case(False)

    def test_interrupted_arm_does_not_start_another_tool(self):
        self._failed_tool_case(True)

    def _failed_tool_case(self, interrupted):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            settings = root / 'settings.json'
            settings.write_text(json.dumps({tool: {'model_tiers': {'standard': {'model': 'fixture', 'effort': 'low'}}} for tool in ['codex', 'claude']}))
            binary = root / 'macc'
            binary.write_text('fixture binary; never executed')
            calls = []
            def fail(_commands, _macc, _output, tool, _settings, case, arm, *_rest, **_kwargs):
                calls.append((tool, case, arm))
                return {'tool': tool, 'case': case, 'arm': arm, 'accepted': False, 'interrupted': interrupted, 'error': 'interrupted' if interrupted else 'quota', 'metrics': metrics.measured([])}
            with patch.object(cli, 'exercise', side_effect=fail), patch.object(Commands, 'run', return_value='fixture-version'), contextlib.redirect_stdout(io.StringIO()):
                code = cli.main(['--settings', str(settings), '--macc', str(binary), '--execute',
                                 '--case', 'slug-list', '--case', 'bounded-values', '--first-arm', 'reuse',
                                 '--output', str(root / 'result'), '--archive-root', str(root / 'archives')])
            self.assertEqual(code, 1)
            expected = [('codex', 'slug-list', 'reuse')]
            if not interrupted:
                expected.append(('claude', 'slug-list', 'reuse'))
            self.assertEqual(calls, expected)
            self.assertEqual(json.loads((root / 'result/report.json').read_text())['status'], 'failed')
            self.assertTrue((root / 'archives/result/archive_manifest.json').is_file())

    def test_public_cli_workflow_with_a_future_adapter(self):
        self._public_cli_workflow('session-reuse')

    def test_memory_context_uses_real_review_and_injection_with_a_future_adapter(self):
        self._public_cli_workflow('memory-context')

    def test_output_reduction_preserves_real_failure_diagnostics(self):
        self._public_cli_workflow('output-reduction')

    def test_navigation_uses_public_index_and_current_worktree_queries(self):
        self._public_cli_workflow('native-navigation')

    def test_navigation_rejects_unsupported_or_unparsed_evidence(self):
        with self.assertRaisesRegex(RuntimeError, 'unsupported'):
            navigation_experiment.query(lambda *_args: '{"outcome":"unsupported","data":{}}', '/fixture', 'charge-policy', True)
        response = {'provider_id': 'native', 'hits': [{'path': 'policies/billing.py', 'file_sha256': 'known', 'provenance': 'literal'}]}
        with patch.object(navigation_experiment, 'query', return_value={'response': response}):
            with self.assertRaisesRegex(RuntimeError, 'uniquely parsed'):
                navigation_experiment.evidence(None, '/fixture', 'charge-policy', {'indexed': True, 'queries': []})

    def _public_cli_workflow(self, feature):
        macc = cli.SOURCE / 'target/debug/macc'
        if not macc.is_file():
            self.skipTest('Build macc before the public CLI integration test')
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            fake = root / 'fixture-ai'
            fake.write_text(f'''#!{sys.executable}
import json, pathlib, re, sys, subprocess
if '--version' in sys.argv:
    print('fixture-ai 1.0')
    raise SystemExit(0)
args = sys.argv[1:]
mode = 'resume' if 'resume' in args else 'create'
sid = args[args.index(mode) + 1]
prompt = args[-1]
ack = re.search(r'^MACC_CHECKPOINT_ACK: .*$', prompt, re.M).group(0)
body = 'def slug(text): return "-".join(text.lower().split())\\n'
if 'Implement unique_slugs' in prompt.split('## MACC task checkpoint')[0]:
    body += 'def unique_slugs(values): return list(dict.fromkeys(filter(None, map(slug, values))))\\n'
else:
    body += 'def unique_slugs(values): raise NotImplementedError\\n'
plain = prompt.split('## MACC task checkpoint')[0]
if 'Retrieve the approved policy through MCP' in plain:
    response = subprocess.run([sys.executable, '.macc/memory_mcp_client.py', 'search', 'policy'], check=True, capture_output=True, text=True)
    page = json.loads(json.loads(response.stdout)['content'][0]['text'])
    memory_id = page['records'][0]['memory']['id']
    subprocess.run([sys.executable, '.macc/memory_mcp_client.py', 'get', memory_id], check=True, capture_output=True)
if 'Implement reference_code' in plain or 'Implement format_references' in plain:
    body = 'def reference_code(namespace, number):\\n namespace = namespace.strip().lower()\\n number = number.strip()\\n if not namespace or not namespace.isascii() or not namespace.isalpha() or not number or not number.isascii() or not number.isdigit(): raise ValueError()\\n return namespace + "::" + number\\n'
    if 'Implement format_references' in plain:
        body += 'def format_references(rows): return [reference_code(a, b) for a, b in rows]\\n'
    else:
        body += 'def format_references(rows): raise NotImplementedError\\n'
if 'ChargePolicy' in plain or 'Implement evaluate_all' in plain:
    body = 'from policies.billing import ChargePolicy\\n\\ndef evaluate(value): return ChargePolicy.evaluate(value)\\n'
    body += ('def evaluate_all(values): return [evaluate(value) for value in values]\\n' if 'Implement evaluate_all' in plain else 'def evaluate_all(values): raise NotImplementedError\\n')
pathlib.Path('solution.py').write_text(body)
print(json.dumps({{'type':'result','session_id':sid,'benchmark_context_seen':'## MACC optional source-backed memory' in prompt,'text':ack+'\\nMACC_TASK_RESULT: success_with_changes','usage':{{'input':100,'cached':20,'output':10}}}}))
''')
            fake.chmod(0o755)
            runner = root / 'runner.sh'
            runner.write_text('export TOOL_ID=fixture-ai TOOL_LOG_PREFIX=fixture\nsource "' + str(cli.SOURCE / 'adapters/shared/performer_lib.sh') + '"\n')
            spec = {'api_version': 'v1', 'id': 'fixture-ai', 'display_name': 'Fixture', 'fields': [],
                    'performer': {'runner': str(runner), 'command': str(fake), 'args': ['--model', 'fixture', 'create', '{session_id}'],
                                  'prompt': {'mode': 'arg'},
                                  'session': {'enabled': True, 'scope': 'task_role_tool', 'id_strategy': 'generated',
                                              'create': {'command': str(fake), 'args': ['--model', 'fixture', 'create', '{session_id}']},
                                              'resume': {'command': str(fake), 'args': ['--model', 'fixture', 'resume', '{session_id}']}},
                                  'compaction': {'schema': 'fixture-v1', 'version_pattern': '^fixture-ai 1.0', 'documentation': 'https://example.invalid/fixture'},
                                  'usage': {'schema': 'fixture-usage-v1', 'event_match': {'/type': 'result'},
                                            'text_match': {'/type': 'result'}, 'text': ['/text'], 'session_id': '/session_id',
                                            'token_scope': 'invocation', 'cost_scope': 'invocation',
                                            'fields': {'input_uncached': '/usage/input', 'cached_read': '/usage/cached', 'output': '/usage/output'}}},
                    'model_tiers': {'standard': {'model': 'fixture', 'effort': 'low'}}}
            spec_path = root / 'spec.json'
            spec_path.write_text(json.dumps(spec))
            settings = root / 'settings.json'
            settings.write_text(json.dumps({'fixture-ai': {'model_tiers': spec['model_tiers']}}))
            with contextlib.redirect_stdout(io.StringIO()):
                exit_code = cli.main(['--settings', str(settings), '--tool', 'fixture-ai', '--tool-spec', str(spec_path),
                                      '--feature', feature, '--execute', '--output', str(root / 'result'),
                                      '--archive-root', str(root / 'archive')])
            report = json.loads((root / 'result/report.json').read_text())
            failures = [Path(entry['log']).read_text() for entry in json.loads((root / 'result/commands.json').read_text()) if entry['status'] != 'ok']
            self.assertEqual(exit_code, 0, '\n'.join(failures) or report)
            self.assertEqual(len(report['arms']), 2)
            self.assertTrue(all(arm['session_behavior_verified'] for arm in report['arms']))
            self.assertTrue(report['pairs'][0]['eligible'], report)
            self.assertEqual(sum(len(arm['calls']) for arm in report['arms']), 4)
            self.assertTrue((root / 'archive/result/archive_manifest.json').is_file())
            if feature == 'output-reduction':
                arms = {arm['arm']: arm for arm in report['arms']}
                self.assertEqual(arms['full']['comparison_config_sha256'], arms['reduced']['comparison_config_sha256'])
                for arm in report['arms']:
                    state = arm['output_experiment']
                    self.assertTrue(state['verified'])
                    self.assertEqual(len(state['outputs']), 2)
                    for row in state['outputs']:
                        self.assertEqual(row['exit_code'], 1)
                        self.assertTrue(Path(row['raw_path']).is_file())
                        self.assertEqual(row['shown_bytes'] < row['raw_bytes'], state['enabled'])
            if feature == 'native-navigation':
                arms = {arm['arm']: arm for arm in report['arms']}
                self.assertEqual(arms['literal']['comparison_config_sha256'], arms['indexed']['comparison_config_sha256'])
                self.assertNotEqual(arms['literal']['config_sha256'], arms['indexed']['config_sha256'])
                for arm in report['arms']:
                    state = arm['navigation_experiment']
                    self.assertTrue(state['verified'])
                    self.assertEqual(len(state['queries']), 2)
                    self.assertEqual(len(state['warm_probes']), 4)
                    self.assertTrue(all(q['response']['provider_id'] == 'native' for q in state['queries']))
                    if state['indexed']:
                        self.assertEqual(len(state['queries'][0]['response']['hits']), 1)
                    else:
                        self.assertGreater(len(state['queries'][0]['response']['hits']), 1)
            if feature == 'memory-context':
                arms = {arm['arm']: arm for arm in report['arms']}
                self.assertEqual(arms['no-push']['memory_experiment']['bundles'], [])
                self.assertEqual(len(arms['push']['memory_experiment']['bundles']), 2)
                self.assertNotEqual(arms['no-push']['config_sha256'], arms['push']['config_sha256'])
                self.assertEqual(arms['no-push']['comparison_config_sha256'], arms['push']['comparison_config_sha256'])
                commands = json.loads((root / 'result/commands.json').read_text())
                for arm in report['arms']:
                    for call in arm['calls']:
                        raw = Path(commands[call['command_index']]['log']).read_text()
                        events = []
                        for line in raw.splitlines():
                            try:
                                event = json.loads(line)
                            except ValueError:
                                continue
                            if 'benchmark_context_seen' in event:
                                events.append(event['benchmark_context_seen'])
                        self.assertEqual(events, [arm['arm'] == 'push'])

    def test_archive_never_overwrites_and_keeps_failure_evidence(self):
        with tempfile.TemporaryDirectory() as name:
            source = Path(name) / 'failed-run'
            source.mkdir()
            (source / 'report.json').write_text('{"status":"failed"}')
            target = retention.reserve(source, Path(name) / 'archives')
            retention.preserve(source, target)
            self.assertEqual((target / 'report.json').read_text(), '{"status":"failed"}')
            self.assertTrue(json.loads((target / 'archive_manifest.json').read_text())['complete'])
            with self.assertRaises(FileExistsError):
                retention.reserve(source, Path(name) / 'archives')
            with self.assertRaises(FileExistsError):
                retention.preserve(source, target)

    def test_memory_pair_requires_matching_corpus_and_observed_injection(self):
        rows = [{'incremental': {'input_uncached': 20}, 'model': 'fixed', 'provider_version': '1',
                 'status': 'recorded', 'outcome': 'success'}] * 2
        baseline = {'tool': 'future-ai', 'case': 'reference-code', 'feature': 'memory-context',
                    'arm': 'no-push', 'accepted': True, 'session_behavior_verified': True,
                    'metrics': metrics.measured(rows), 'starting_revision': 'fixed-revision',
                    'profile_sha256': 'fixed-profile', 'comparison_config_sha256': 'masked-context-flag-only',
                    'memory_experiment': {'enabled': False, 'verified': True, 'corpus_sha256': 'fixed-corpus'}}
        candidate = copy.deepcopy(baseline)
        candidate['arm'] = 'push'
        candidate['memory_experiment']['enabled'] = True
        self.assertTrue(metrics.compare([baseline, candidate])[0]['eligible'])
        candidate['memory_experiment']['corpus_sha256'] = 'different-corpus'
        self.assertFalse(metrics.compare([baseline, candidate])[0]['eligible'])
        candidate['memory_experiment']['corpus_sha256'] = 'fixed-corpus'
        candidate['memory_experiment']['verified'] = False
        self.assertFalse(metrics.compare([baseline, candidate])[0]['eligible'])

    def test_user_interruption_stops_process_group_and_preserves_evidence(self):
        with tempfile.TemporaryDirectory() as name:
            commands = Commands(name)
            with patch('benchmarks.o2.transport.subprocess.Popen') as popen, patch('benchmarks.o2.transport.terminate_group') as stop:
                popen.return_value.communicate.side_effect = KeyboardInterrupt()
                popen.return_value.returncode = -15
                with self.assertRaises(ExperimentInterrupted):
                    commands.run(['fixture-ai'], name)
                stop.assert_called_once_with(popen.return_value)
            rows = json.loads((Path(name) / 'commands.json').read_text())
            self.assertEqual(rows[0]['status'], 'interrupted')


if __name__ == '__main__':
    unittest.main()
