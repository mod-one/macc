"""Public managed-provider lifecycle and current-source navigation experiments."""
import json
import shutil
import time

from .memory_experiment import fingerprint
from .navigation_cases import CASES

PROVIDERS = {'graphify-navigation': 'graphify', 'serena-navigation': 'serena'}


def configure(config, feature, candidate):
    provider = PROVIDERS[feature]
    config['settings']['offline'] = False  # Only explicit install/update may download.
    code = config['code_intelligence']
    code.update(enabled=True, routing='adaptive' if candidate else 'native_only',
                native_map_enabled=provider == 'serena')
    code.update({f'{provider}_enabled': True, f'{provider}_python': shutil.which('python3'),
                 f'{provider}_uv': shutil.which('uv')})
    if not code[f'{provider}_python'] or not code[f'{provider}_uv']:
        raise RuntimeError('Managed provider requires existing Python 3.12 and uv')


def operation(invoke, provider, action):
    started = time.monotonic()
    preview = json.loads(invoke('o2', 'provider-plan', provider, action, timeout=60))
    if preview['result']['outcome'] != 'supported':
        raise RuntimeError(f'{provider} does not support {action}')
    plan = preview['result']['data']
    receipt = json.loads(invoke('o2', 'provider-apply', plan['id'], preview['checksum'], timeout=900))
    return {'wall_ms': round((time.monotonic() - started) * 1000, 3),
            'plan': preview, 'receipt': receipt}


def query(invoke, wt, case, state, require_provider=True):
    argv = ['o2', 'code-query', CASES[case]['symbol'], '--kind', state['kind'], '--worktree', str(wt)]
    if state['provider'] == 'serena':
        argv += ['--path', CASES[case]['definition_path']]
    started = time.monotonic()
    envelope = json.loads(invoke(*argv, timeout=150))
    if envelope.get('outcome') != 'supported':
        raise RuntimeError('Navigation request unsupported: ' + json.dumps(envelope))
    response = envelope['data']
    expected = state['provider'] if state['candidate'] else 'native'
    if response.get('snapshot_mismatch') or (require_provider and response.get('provider_id') != expected):
        raise RuntimeError(f'Expected {expected} evidence; fallback cannot establish a provider saving')
    if require_provider:
        target = CASES[case]['definition_path']
        hits = response.get('hits', [])
        if not any(h.get('path') == target and h.get('file_sha256') for h in hits):
            raise RuntimeError('Provider evidence omitted the authoritative policy source')
        if state['candidate'] and not all(h.get('provenance', '').lower().startswith(state['provider']) for h in hits):
            raise RuntimeError('Managed provider provenance is missing')
    return {'wall_ms': round((time.monotonic() - started) * 1000, 3), 'response': response}


def prepare(invoke, root, wt, config, case, feature, candidate):
    provider = PROVIDERS[feature]
    status = json.loads(invoke('o2', 'status'))
    if not status['ready']:
        plan = json.loads(invoke('o2', 'upgrade'))
        invoke('o2', 'upgrade', '--apply', '--expected-sha256', plan['source_sha256'])
    policy = json.loads(invoke('o2', 'status'))['policy']
    code = policy['code_intelligence']
    if not code['enabled'] or not code[f'{provider}_enabled'] or code['routing'] != ('adaptive' if candidate else 'native_only'):
        raise RuntimeError('Managed-provider policy differs from the treatment')
    if any(policy[k]['enabled'] for k in ['memory', 'context_engine', 'adaptive_compaction']):
        raise RuntimeError('Other O2 treatments must remain disabled')
    state = {'provider': provider, 'candidate': candidate, 'verified': False,
             'kind': 'references' if provider == 'graphify' else 'symbol_definition',
             'lifecycle': [], 'calibration_queries': [], 'task_queries': [],
             'corpus_sha256': fingerprint(CASES[case])}
    if provider == 'serena':
        # Compare Jedi with a prepared native map, rather than repeated cold parsing.
        state['native_lifecycle'] = [operation(invoke, 'native', action) for action in ['deploy', 'index']]
    if candidate:
        for action in (['install', 'deploy', 'index'] if provider == 'graphify' else ['install', 'deploy']):
            state['lifecycle'].append(operation(invoke, provider, action))
        if state['lifecycle'][-1]['receipt']['state'] != 'healthy':
            raise RuntimeError('Managed provider did not become healthy')
    for _ in range(2):
        state['calibration_queries'].append(query(invoke, wt, case, state))
    normalized = json.loads(json.dumps(config))
    normalized['code_intelligence']['routing'] = 'native_only'
    policy['code_intelligence']['routing'] = 'native_only'
    state['preparation_verified'] = True
    return state, fingerprint({'bootstrap': normalized, 'effective_o2': policy})


def evidence(invoke, wt, case, state, step):
    if step:
        return '\nReuse the navigation evidence from step one; inspect current source before editing.\n'
    result = query(invoke, wt, case, state)
    state['task_queries'].append(result)
    return '\nPublic MACC navigation evidence (source remains authoritative):\n' + json.dumps(result['response']) + '\n'


def inspect(invoke, wt, case, state):
    state['verified'] = len(state['task_queries']) == 1 and state['preparation_verified']
    if not state['verified']:
        raise RuntimeError('Initial current-source provider evidence was not delivered')
    # No stale Graphify result is fed to the model. This post-task probe is separate.
    if state['candidate']:
        state['after_edit_query'] = query(invoke, wt, case, state, require_provider=False)
        actual = state['after_edit_query']['response']['provider_id']
        if actual != ('native' if state['provider'] == 'graphify' else 'serena'):
            raise RuntimeError('Dirty-source provider behavior did not match its freshness contract')
