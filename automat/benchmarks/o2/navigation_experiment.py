"""Public native lifecycle/query measurements and controlled navigation evidence."""
import json
import time

from .memory_experiment import fingerprint
from .navigation_cases import CASES


def operation(invoke, action):
    plan = json.loads(invoke('o2', 'provider-plan', 'native', action))
    if plan['result']['outcome'] != 'supported':
        raise RuntimeError('Native lifecycle operation unsupported')
    return json.loads(invoke('o2', 'provider-apply', plan['result']['data']['id'], plan['checksum']))


def query(invoke, wt, case, indexed):
    started = time.monotonic()
    envelope = json.loads(invoke('o2', 'code-query', CASES[case]['symbol'], '--kind',
                                 'symbol_definition' if indexed else 'references', '--worktree', str(wt)))
    if envelope.get('outcome') != 'supported':
        raise RuntimeError('Public navigation query is unsupported: ' + json.dumps(envelope))
    response = envelope['data']
    return {'wall_ms': round((time.monotonic() - started) * 1000, 3), 'response': response}


def prepare(invoke, root, wt, config, case, indexed):
    status = json.loads(invoke('o2', 'status'))
    if not status['ready']:
        plan = json.loads(invoke('o2', 'upgrade'))
        invoke('o2', 'upgrade', '--apply', '--expected-sha256', plan['source_sha256'])
    policy = json.loads(invoke('o2', 'status'))['policy']
    code = policy['code_intelligence']
    if not code['enabled'] or code['native_map_enabled'] is not indexed:
        raise RuntimeError('Navigation policy differs from the assigned treatment')
    if policy['memory']['enabled'] or policy['context_engine']['enabled'] or policy['adaptive_compaction']['enabled']:
        raise RuntimeError('Other O2 treatments must remain disabled')
    state = {'indexed': indexed, 'verified': False, 'queries': [], 'warm_probes': []}
    state['query_without_persisted_index'] = query(invoke, wt, case, indexed)
    if indexed:
        state['deploy'] = operation(invoke, 'deploy')
        for action in ['index', 'update']:
            started = time.monotonic()
            receipt = operation(invoke, action)
            state[action] = {'wall_ms': round((time.monotonic() - started) * 1000, 3), 'receipt': receipt}
            metadata = receipt.get('metadata', {})
            stats = metadata.get('stats', {})
            if receipt.get('state') != 'healthy' or metadata.get('truncated') is not False or stats.get('failed_files') != 0:
                raise RuntimeError('Native preparation is incomplete or degraded')
            if action == 'index' and not stats.get('parsed_files', 0) > 0:
                raise RuntimeError('Initial index did not parse the tracked fixture')
            if action == 'update' and (stats.get('parsed_files') != 0 or stats.get('reused_files') != state['index']['receipt']['metadata']['stats']['parsed_files']):
                raise RuntimeError('Unchanged update did not reuse the initial parsed corpus')
    # First query and repeated reads are kept separate; OS/provider caches are not flushed.
    for _ in range(4):
        state['warm_probes'].append(query(invoke, wt, case, indexed))
    normalized = json.loads(json.dumps(config))
    normalized['code_intelligence']['native_map_enabled'] = False
    policy['code_intelligence']['native_map_enabled'] = False
    state['corpus_sha256'] = fingerprint(CASES[case]['files'])
    return state, fingerprint({'bootstrap': normalized, 'effective_o2': policy})


def evidence(invoke, wt, case, state):
    result = query(invoke, wt, case, state['indexed'])
    state['queries'].append(result)
    response = result['response']
    # Fail closed on unsupported/mismatched/truncated output; never relabel a fallback.
    if response.get('provider_id') != 'native' or response.get('snapshot_mismatch'):
        raise RuntimeError('Expected current-source native navigation evidence')
    hits = response.get('hits', [])
    target = CASES[case]['definition_path']
    if not any(hit.get('path') == target and hit.get('file_sha256') for hit in hits):
        raise RuntimeError('Navigation evidence omitted the authoritative definition')
    if state['indexed']:
        diagnostics = ' '.join(response.get('diagnostics', []))
        if len(hits) != 1 or 'map_truncated=false' not in diagnostics or 'failed=0,' not in diagnostics or not hits[0].get('provenance', '').startswith('native:tree-sitter:'):
            raise RuntimeError('Native definition was not uniquely parsed with complete map coverage')
    # The same prompt framing is used in both arms; only the public query result differs.
    return '\nNavigation evidence from the public MACC CLI (inspect source before editing):\n' + json.dumps(response) + '\n'


def inspect(state):
    state['verified'] = len(state['queries']) == 2
    if not state['verified']:
        raise RuntimeError('Both task steps must receive verified navigation results')
