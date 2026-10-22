"""Compare automatic push with real agent-invoked stdio MCP pull."""
import hashlib
import json
from pathlib import Path

from . import memory_experiment


def prepare(invoke, root, wt, config, case, task, push):
    state, comparable = memory_experiment.prepare(invoke, root, config, case, task, push)
    client = wt / '.macc/memory_mcp_client.py'
    client.write_bytes(Path(__file__).with_name('mcp_client.py').read_bytes())
    state.update(mode='push' if push else 'pull', client_sha256=hashlib.sha256(client.read_bytes()).hexdigest())
    return state, comparable


def instruction(state, step):
    if state['enabled']:
        return '\nUse the automatically injected approved policy. Do not invoke the MCP client in this push variant.\n'
    if step == 0:
        return ('\nRetrieve the approved policy through MCP before editing: run '
                '`python3 .macc/memory_mcp_client.py search "policy"`. '
                'Read the returned approved record; if only references are returned, use '
                '`python3 .macc/memory_mcp_client.py get MEMORY_ID`. '
                'This client is the explicitly allowed .macc reader for this task. '
                'If MCP fails, report the failure rather than claiming it was used.\n')
    return '\nReuse the approved policy retrieved via MCP in this session; call the same client again only if needed.\n'


def inspect(invoke, wt, state, records):
    memory_experiment.inspect(invoke, state, records)
    client = wt / '.macc/memory_mcp_client.py'
    if hashlib.sha256(client.read_bytes()).hexdigest() != state['client_sha256']:
        raise RuntimeError('Scoped MCP client changed during execution')
    path = wt / '.macc/benchmark-mcp-events.jsonl'
    events = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
    state['mcp_events'] = events
    if state['enabled']:
        if events:
            raise RuntimeError('Push baseline invoked MCP; comparison not isolated')
        return
    sessions = {(r.get('logical_session_id'), str(r.get('generation'))) for r in records}
    found = False
    for event in events:
        if event.get('error') or (event.get('session_id'), event.get('generation')) not in sessions:
            raise RuntimeError('MCP call failed or was not attributed to the measured session')
        for exchange in event['exchanges']:
            if exchange['request']['method'] != 'tools/call':
                continue
            response = exchange['response']['result']
            if response.get('isError'):
                raise RuntimeError('MCP tool response failed')
            for content in response.get('content', []):
                data = json.loads(content['text'])
                for hit in data.get('records', []):
                    if hit.get('memory', {}).get('id') == state['memory_id'] and hit.get('checksum') == state['memory_checksum'] and hit['memory']['proposal'].get('body') == state['proposal']['body']:
                        found = True
    state['verified'] = found
    if not found:
        raise RuntimeError('No actual live MCP response delivered the complete approved benchmark policy')
