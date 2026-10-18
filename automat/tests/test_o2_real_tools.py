#!/usr/bin/env python3
"""Normal validation never launches a real AI CLI."""
import contextlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import o2_real_tools as smoke


class RealToolPlanTests(unittest.TestCase):
    def test_four_tools_plan_without_probes_or_model_calls(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            args = ['--report', str(root / 'plan.json')]
            for tool in ['claude', 'codex', 'gemini', 'vibe']:
                path = root / f'{tool}.json'
                path.write_text(json.dumps({'id': tool, 'performer': {'command': 'never-launch-this'}}))
                args += ['--tool-json', str(path)]
            with patch.object(smoke, 'exercise', side_effect=AssertionError('paid calls forbidden')):
                self.assertEqual(smoke.main(args), 0)
            plan = json.loads((root / 'plan.json').read_text())
            self.assertEqual(plan['mode'], 'plan')
            self.assertEqual(plan['maximum_model_calls'], 20)
            self.assertEqual(len(plan['tools']), 4)
            self.assertEqual((root / 'plan.json').stat().st_mode & 0o777, 0o600)
            # Never overwrite evidence on retry.
            with self.assertRaises(FileExistsError):
                smoke.main(args)

    def test_execution_requires_explicit_consent_before_reading_profiles(self):
        with contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit):
                smoke.main(['--tool-json', '/missing', '--report', '/missing', '--execute'])

    def test_unresolved_model_or_duplicate_identity_cannot_launch(self):
        with tempfile.TemporaryDirectory() as name:
            path = Path(name) / 'tool.json'
            path.write_text(json.dumps({'id': 'future-ai', 'performer': {'command': 'future', 'args': ['{model}']}}))
            with self.assertRaises(ValueError):
                smoke.profile(path)

    def test_timeout_terminates_process_group_without_retry(self):
        with tempfile.TemporaryDirectory() as root:
            with self.assertRaisesRegex(RuntimeError, 'timed out'):
                smoke.run(['python3', '-c', 'import time; time.sleep(60)'], root, {}, timeout=0.05)


if __name__ == '__main__':
    unittest.main()
