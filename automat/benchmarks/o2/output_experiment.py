"""Controlled full/reduced diagnostics through public CLI; never rewrite usage."""
import hashlib
import json
from pathlib import Path
import time

from .memory_experiment import fingerprint
from .output_cases import CASES


def prepare(invoke, config, case, enabled):
    status = json.loads(invoke('o2', 'status'))
    if not status['ready']:
        plan = json.loads(invoke('o2', 'upgrade'))
        invoke('o2', 'upgrade', '--apply', '--expected-sha256', plan['source_sha256'])
    policy = json.loads(invoke('o2', 'status'))['policy']
    if policy['context_engine']['output_reduction'] is not enabled:
        raise RuntimeError('Output policy differs from the assigned treatment')
    if any(policy[key]['enabled'] for key in ['memory', 'context_engine', 'code_intelligence', 'adaptive_compaction']):
        raise RuntimeError('Other O2 treatments must remain disabled')
    normalized = json.loads(json.dumps(config))
    normalized['context_engine']['output_reduction'] = False
    policy['context_engine']['output_reduction'] = False
    return ({'enabled': enabled, 'verified': False, 'outputs': [],
             'corpus_sha256': fingerprint(CASES[case])},
            fingerprint({'bootstrap': normalized, 'effective_o2': policy}))


def essential(raw):
    # Keep the test identity, file, exception/assertion message and final verdict.
    required = ['test_requested_behavior', 'diagnostics.py', 'FAILED']
    exception = 'AssertionError' if 'AssertionError' in raw else 'NotImplementedError'
    required.append(exception)
    required.extend(line for line in raw.splitlines() if line.startswith('AssertionError:'))
    return required


def evidence(commands, invoke, root, wt, case, step, state):
    result = json.loads(commands.run(['python3', '-B', 'diagnostics.py', case, str(step)], wt))
    if result['exit_code'] != 1 or result['tests_run'] != 201:
        raise RuntimeError('Expected a real failing contract check and 200 passing background tests')
    raw = result['output']
    raw_hash = hashlib.sha256(raw.encode()).hexdigest()
    saved = root / f'.macc/log/benchmark-diagnostics-{step}.txt'
    saved.parent.mkdir(parents=True, exist_ok=True)
    saved.write_text(raw)
    started = time.monotonic()
    reduced = json.loads(invoke('o2', 'reduce', 'test', input_text=raw)) if state['enabled'] else None
    shown = reduced['summary'] if reduced else raw
    reference = (root / reduced['raw_ref']).resolve() if reduced else saved.resolve()
    if not reference.is_relative_to((root / '.macc/log').resolve()) or hashlib.sha256(reference.read_bytes()).hexdigest() != raw_hash:
        raise RuntimeError('Raw output artifact is missing, out of scope or changed')
    required = essential(raw)
    if not all(token in raw and token in shown for token in required):
        raise RuntimeError('Essential failure diagnostics were lost during output reduction')
    if reduced and (len(shown.encode()) >= len(raw.encode()) or reduced['estimated_tokens'] > 2000):
        raise RuntimeError('Reduced response did not shrink or exceeded the reviewed budget')
    record = {'step': step + 1, 'exit_code': result['exit_code'], 'tests_run': result['tests_run'],
              'raw_bytes': len(raw.encode()), 'shown_bytes': len(shown.encode()),
              'raw_sha256': raw_hash, 'shown_sha256': hashlib.sha256(shown.encode()).hexdigest(),
              'raw_path': str(reference), 'essential_markers': required,
              'elapsed_ms': round((time.monotonic() - started) * 1000, 3), 'reduced': reduced}
    state['outputs'].append(record)
    return ('\nActual Python contract-test output (test exit code 1; 201 tests run). '
            'The exact raw artifact below may be read if needed, despite the general log restriction.\n'
            f'Raw artifact: {reference}\n' + shown + '\n')


def inspect(state):
    state['verified'] = len(state['outputs']) == 2
    if not state['verified']:
        raise RuntimeError('Both steps must receive checked real diagnostic output')
