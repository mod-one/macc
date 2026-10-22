"""One-shot stdio MCP reader, executed by the agent under its inherited live lease.

Never serialize environment values or capability credentials. The JSONL evidence
contains public requests/responses and session identity only.
"""
import json
import os
from pathlib import Path
import selectors
import subprocess
import sys
import time


def main(argv=None):
    args = list(sys.argv[1:] if argv is None else argv)
    if not args or args[0] not in ['search', 'get'] or len(args) != 2:
        raise RuntimeError('Usage: memory_mcp_client.py search QUERY | get MEMORY_ID')
    required = ['MACC_O2_PROJECT_ROOT', 'MACC_O2_MCP_SESSION', 'MACC_O2_MCP_GENERATION', 'MACC_O2_MCP_TOKEN']
    if any(not os.environ.get(key) for key in required):
        raise RuntimeError('MCP reader requires the active MACC runner session')
    audit = Path('.macc/benchmark-mcp-events.jsonl')
    method = 'memory.search' if args[0] == 'search' else 'memory.get'
    arguments = {'query': args[1], 'limit': 1} if args[0] == 'search' else {'id': args[1], 'max_chars': 4096}
    started = time.monotonic()
    process = subprocess.Popen([os.environ.get('MACC_SESSION_SERVICE_BIN', 'macc'), 'o2', 'mcp'],
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    exchanges = []
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    def send(request, expect=True):
        wire = (json.dumps(request, separators=(',', ':')) + '\n').encode()
        process.stdin.write(wire)
        process.stdin.flush()
        if not expect:
            return None
        if not selector.select(10):
            raise RuntimeError('Scoped MCP response timed out')
        line = process.stdout.readline(262145)
        if not line or len(line) > 262144:
            raise RuntimeError('Scoped MCP response missing or oversized')
        response = json.loads(line)
        if response.get('id') != request['id'] or response.get('jsonrpc') != '2.0':
            raise RuntimeError('Scoped MCP response framing mismatch')
        exchanges.append({'request': request, 'response': response, 'request_bytes': len(wire), 'response_bytes': len(line)})
        if 'error' in response:
            raise RuntimeError('Scoped MCP protocol rejected the request')
        return response
    error = None
    response = None
    try:
        send({'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {'protocolVersion': '2025-11-25', 'capabilities': {}, 'clientInfo': {'name': 'macc-benchmark-reader', 'version': '1'}}})
        send({'jsonrpc': '2.0', 'method': 'notifications/initialized'}, False)
        response = send({'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call', 'params': {'name': method, 'arguments': arguments}})
        if response.get('result', {}).get('isError') is not False:
            raise RuntimeError('Scoped memory read was rejected')
    except (OSError, ValueError, RuntimeError) as failure:
        error = str(failure)
    finally:
        selector.close()
        process.stdin.close()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)
        process.stdout.close()
        record = {'method': method, 'arguments': arguments, 'elapsed_ms': round((time.monotonic() - started) * 1000, 3),
                  'session_id': os.environ['MACC_O2_MCP_SESSION'], 'generation': os.environ['MACC_O2_MCP_GENERATION'],
                  'exchanges': exchanges, 'error': error}
        with audit.open('a') as stream:
            stream.write(json.dumps(record) + '\n')
    if error:
        raise RuntimeError(error)
    print(json.dumps(response['result']))
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, RuntimeError):
        print('Scoped MCP reader failed; inspect redacted benchmark events.', file=sys.stderr)
        raise SystemExit(1)
