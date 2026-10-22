"""Reduction regression checks without paid model calls."""
import contextlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from benchmarks.o2 import cli, features, output_checks, output_experiment


class OutputReductionTests(unittest.TestCase):
    def test_public_cli_matrix_preserves_evidence_and_bounds_unicode(self):
        if not (cli.SOURCE / 'target/debug/macc').is_file():
            self.skipTest('Build macc before public CLI checks')
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with contextlib.redirect_stdout(io.StringIO()):
                code = output_checks.main(['--macc', str(cli.SOURCE / 'target/debug/macc'), '--output', str(root / 'checks'), '--archive-root', str(root / 'archive')])
            report = json.loads((root / 'checks/report.json').read_text())
            self.assertEqual(code, 0, report)
            self.assertEqual(report['model_calls'], 0)
            self.assertEqual(len(report['checks']), 6)
            self.assertTrue((root / 'archive/checks/archive_manifest.json').exists())

    def test_missing_failure_message_cannot_pass_treatment(self):
        state = {'enabled': True, 'outputs': []}
        raw = 'FAIL: test_requested_behavior\nFile diagnostics.py\nAssertionError: 1 != 2\nFAILED\n'
        class Commands:
            def run(self, *_args):
                return json.dumps({'exit_code': 1, 'tests_run': 201, 'output': raw})
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            path = root / '.macc/log/reduced.txt'
            path.parent.mkdir(parents=True)
            path.write_text(raw)
            def invoke(*_args, **_kwargs):
                return json.dumps({'summary': 'FAIL test_requested_behavior diagnostics.py FAILED AssertionError', 'raw_ref': '.macc/log/reduced.txt', 'estimated_tokens': 20})
            with self.assertRaisesRegex(RuntimeError, 'Essential failure diagnostics'):
                output_experiment.evidence(Commands(), invoke, root, root, 'slug-diagnostics', 0, state)
            self.assertEqual(state['outputs'], [])

    def test_comparison_requires_same_corpus_and_observed_treatment(self):
        baseline = {'feature': 'output-reduction', 'output_experiment': {'enabled': False, 'verified': True, 'corpus_sha256': 'same'}}
        candidate = {'feature': 'output-reduction', 'output_experiment': {'enabled': True, 'verified': True, 'corpus_sha256': 'same'}}
        self.assertTrue(features.treatment_pair_verified(baseline, candidate))
        candidate['output_experiment']['verified'] = False
        self.assertFalse(features.treatment_pair_verified(baseline, candidate))


if __name__ == '__main__':
    unittest.main()
