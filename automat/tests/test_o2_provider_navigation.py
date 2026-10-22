"""Managed-provider benchmark regressions without packages or model calls."""
import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from benchmarks.o2 import cli, provider_navigation, features


class ProviderNavigationTests(unittest.TestCase):
    def test_candidate_fallback_is_never_accepted_as_provider_evidence(self):
        response = {'outcome': 'supported', 'data': {'provider_id': 'native', 'hits': [], 'snapshot_mismatch': False}}
        def invoke(*_args, **_kwargs):
            return json.dumps(response)
        for provider in ['serena', 'graphify']:
            with self.assertRaisesRegex(RuntimeError, 'fallback'):
                provider_navigation.query(invoke, '/fixture', 'charge-policy', {'provider': provider, 'candidate': True, 'kind': 'references'})

    def test_comparison_requires_same_provider_corpus_and_verified_delivery(self):
        a = {'feature': 'graphify-navigation', 'provider_experiment': {'candidate': False, 'provider': 'graphify', 'verified': True, 'corpus_sha256': 'same'}}
        b = {'feature': 'graphify-navigation', 'provider_experiment': {'candidate': True, 'provider': 'graphify', 'verified': True, 'corpus_sha256': 'same'}}
        self.assertTrue(features.treatment_pair_verified(a, b))
        b['provider_experiment']['verified'] = False
        self.assertFalse(features.treatment_pair_verified(a, b))

    def test_preparation_only_cannot_create_model_calls_or_performance_pairs(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            settings = root / 'settings.json'
            settings.write_text(json.dumps({'codex': {'model_tiers': {'standard': {'model': 'fixed', 'effort': 'medium'}}}}))
            binary = root / 'macc'
            binary.write_text('never executed')
            def prepared(*_args, **kwargs):
                self.assertTrue(kwargs['prepare_only'])
                return {'tool': 'codex', 'case': 'charge-policy', 'arm': _args[6], 'accepted': True, 'status': 'prepared', 'calls': [], 'feature': 'graphify-navigation'}
            with patch.object(cli, 'exercise', side_effect=prepared), patch.object(cli.Commands, 'run', return_value='fixture-version'), contextlib.redirect_stdout(io.StringIO()):
                code = cli.main(['--feature', 'graphify-navigation', '--tool', 'codex', '--settings', str(settings), '--macc', str(binary), '--prepare-only', '--output', str(root / 'result')])
            report = json.loads((root / 'result/report.json').read_text())
            self.assertEqual(code, 0)
            self.assertEqual(report['status'], 'prepared')
            self.assertEqual(report['maximum_model_calls'], 0)
            self.assertEqual(report['protocol']['calls_per_arm'], 0)
            self.assertEqual({a['arm'] for a in report['arms']}, {'native', 'provider'})
            self.assertEqual(report['pairs'], [])
            self.assertTrue(all(not a['calls'] for a in report['arms']))


if __name__ == '__main__':
    unittest.main()
