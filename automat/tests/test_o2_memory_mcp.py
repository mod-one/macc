"""Scoped MCP benchmark checks; the fake future adapter never calls an AI API."""
import os
from pathlib import Path
import subprocess
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from benchmarks.o2 import features, cli
import test_o2_benchmark as benchmark_workflow


class MemoryMcpTests(unittest.TestCase):
    def test_real_mcp_under_live_future_adapter_lease(self):
        benchmark_workflow.O2BenchmarkTests(methodName='runTest')._public_cli_workflow('memory-mcp')

    def test_client_requires_live_runner_environment(self):
        env = {k: v for k, v in os.environ.items() if not k.startswith('MACC_O2_')}
        path = cli.SOURCE / 'automat/benchmarks/o2/mcp_client.py'
        result = subprocess.run([sys.executable, str(path), 'search', 'policy'], env=env, text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('MACC_O2_MCP_TOKEN=', result.stderr)

    def test_pair_needs_actual_observed_mcp_and_matching_corpus(self):
        a = {'feature': 'memory-mcp', 'mcp_experiment': {'verified': True, 'mode': 'push', 'corpus_sha256': 'fixed', 'client_sha256': 'reader'}}
        b = {'feature': 'memory-mcp', 'mcp_experiment': {'verified': True, 'mode': 'pull', 'corpus_sha256': 'fixed', 'client_sha256': 'reader'}}
        self.assertTrue(features.treatment_pair_verified(a, b))
        b['mcp_experiment']['verified'] = False
        self.assertFalse(features.treatment_pair_verified(a, b))


if __name__ == '__main__':
    unittest.main(defaultTest='MemoryMcpTests')
