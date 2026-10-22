"""Reviewed memory bootstrap and observed context treatment, through public CLI only."""
import hashlib
import json

from .memory_cases import CASES


def fingerprint(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def prepare(invoke, root, config, case, task, enabled):
    status = json.loads(invoke('o2', 'status'))
    if not status['ready']:
        plan = json.loads(invoke('o2', 'upgrade'))
        invoke('o2', 'upgrade', '--apply', '--expected-sha256', plan['source_sha256'])
    # Both variants receive the same explicitly reviewed fact and authoritative file.
    evidence = json.loads(invoke('o2', 'memory-evidence', 'docs/policy.md'))
    proposal = {'kind': 'constraint', 'title': CASES[case]['memory_title'],
                'body': CASES[case]['policy'].split('\n\n', 1)[1].strip(),
                'scope': 'project', 'scope_ref': None, 'confidence': 1.0,
                'evidence': [evidence], 'tags': ['benchmark', case],
                'source_task': task, 'source_revision': evidence.get('revision')}
    proposed = json.loads(invoke('o2', 'memory-propose', input_text=json.dumps(proposal)))
    hit = json.loads(invoke('o2', 'memory-get', proposed['id'], '--full'))
    review = {'id': proposed['id'], 'status': 'active', 'confirmed': True,
              'expected_checksum': hit['checksum'],
              'reason': 'Synthetic benchmark fact checked against the identical committed policy source'}
    invoke('o2', 'memory-review', input_text=json.dumps(review))
    active = json.loads(invoke('o2', 'memory-get', proposed['id'], '--full'))
    if active['memory']['status'] != 'active':
        raise RuntimeError('Benchmark memory was not activated')
    if enabled:
        preview = json.loads(invoke('o2', 'policy', 'set', 'context_engine.enabled', 'true'))
        invoke('o2', 'policy', 'set', 'context_engine.enabled', 'true', '--apply', '--confirm', '--revision', preview['revision'])
    effective = json.loads(invoke('o2', 'status'))['policy']
    if effective['context_engine']['enabled'] is not enabled or not effective['memory']['enabled']:
        raise RuntimeError('Memory context policy does not match the assigned treatment')
    normalized = json.loads(json.dumps(effective))
    normalized['context_engine']['enabled'] = False
    state = {'enabled': enabled, 'memory_id': proposed['id'], 'memory_checksum': active['checksum'],
             'corpus_sha256': fingerprint(proposal), 'proposal': proposal,
             'policy_path': 'context_engine.enabled', 'verified': False}
    # Only the predeclared context flag is masked; all other effective policy fields must match.
    return state, fingerprint({'bootstrap': config, 'effective_o2': normalized})


def inspect(invoke, state, records):
    bundles = json.loads(invoke('o2', 'context-list'))
    state['bundles'] = bundles
    if not state['enabled']:
        state['verified'] = not bundles and len(records) == 2 and all(row.get('context_bundle_id') is None for row in records)
    else:
        indexed = {bundle['id']: bundle for bundle in bundles}
        state['verified'] = len(records) == 2 and len(bundles) == 2
        for row in records:
            bundle = indexed.get(row.get('context_bundle_id'), {})
            selected = bundle.get('selected', [])
            state['verified'] = state['verified'] and len(selected) == 1 and selected[0]['id'] == state['memory_id'] and selected[0]['checksum'] == state['memory_checksum'] and bundle.get('optional_estimated_tokens', 0) > 0
    if not state['verified']:
        raise RuntimeError('Observed context bundles/usage do not prove the assigned memory treatment')
