"""Local public-CLI reduction checks; no model calls, even when run directly."""
import argparse
import hashlib
import json
from pathlib import Path

from . import retention
from .transport import Commands


SAMPLES = {
    'test': ('test_existing ... ok\n' * 200 + '\nFAIL: test_contract\nTraceback:\n  File "suite.py", line 9\nAssertionError: 7 != 8\n\nFAILED (failures=1)\n', ['test_contract', 'suite.py', 'AssertionError: 7 != 8', 'FAILED']),
    'lint': ('checking module.py\n' * 200 + 'src/example.py:8:3: error E123 invalid assignment\n', ['src/example.py:8:3', 'E123', 'invalid assignment']),
    'trace': ('ValueError: invalid record\n at parse (app/parser.py:12)\n' + ''.join(f' at frame{i} (node_modules/lib{i}.js:1)\n' for i in range(200)), ['ValueError: invalid record', 'app/parser.py:12', 'frames omitted']),
    'diff': ('1 file changed, 200 insertions(+)\ndiff --git a/example.py b/example.py\n' + '+unchanged_example = 1\n' * 200, ['1 file changed', 'a/example.py', 'b/example.py', 'Full diff truncated']),
    'log': ('INFO processing record\n' * 200 + 'WARN cache miss\nERROR cannot parse record\nrecord_id=123\n', ['WARN cache miss', 'ERROR cannot parse record', 'record_id=123']),
    'utf8-budget': ('ERROR: ' + 'é🐾' * 10000, ['ERROR:']),
}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--macc', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--archive-root', type=Path)
    args = parser.parse_args(argv)
    output = args.output.resolve()
    output.mkdir(parents=True, mode=0o700, exist_ok=False)
    archive = retention.reserve(output, args.archive_root)
    commands = Commands(output)
    report = {'status': 'running', 'model_calls': 0, 'checks': []}
    cli = [str(args.macc.resolve()), '--cwd', str(output)]
    try:
        commands.run(cli + ['init', '--fresh', '--no-restore-prompt'], output)
        (output / '.macc/macc.yaml').write_text(json.dumps({'tools': {'enabled': [], 'config': {}}, 'settings': {'offline': True}, 'context_engine': {'enabled': False, 'output_reduction': True, 'pull_limit_tokens': 2000, 'max_output_bytes': 65536}}))
        for name, (raw, required) in SAMPLES.items():
            response = json.loads(commands.run(cli + ['o2', 'reduce', 'log' if name == 'utf8-budget' else name], output, input_text=raw))
            summary = response['summary']
            reference = (output / response['raw_ref']).resolve()
            if not reference.is_relative_to(output / '.macc/log') or reference.read_bytes() != raw.encode():
                raise RuntimeError('Raw artifact not preserved exactly: ' + name)
            if not all(token in summary for token in required) or response['raw_ref'] not in summary:
                raise RuntimeError('Essential diagnostics/raw pointer missing: ' + name)
            if response['estimated_tokens'] > 2000 or len(summary.encode()) > 8000:
                raise RuntimeError('Complete UTF-8 summary exceeds output budget: ' + name)
            if len(summary.encode()) >= len(raw.encode()):
                raise RuntimeError('Representative noisy output did not shrink: ' + name)
            report['checks'].append({'name': name, 'raw_bytes': len(raw.encode()), 'summary_bytes': len(summary.encode()),
                                     'raw_sha256': hashlib.sha256(raw.encode()).hexdigest(), 'required': required, 'response': response})
        report['status'] = 'passed'
    except (OSError, ValueError, RuntimeError, KeyError) as error:
        report.update(status='failed', error=str(error))
    finally:
        commands.save()
        (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        retention.preserve(output, archive)
    print(output / 'report.json')
    return int(report['status'] != 'passed')


if __name__ == '__main__':
    raise SystemExit(main())
