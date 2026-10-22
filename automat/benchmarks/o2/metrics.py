"""Compare paired results without converting missing telemetry into zero."""
DIMENSIONS = ('input_uncached', 'cached_read', 'cached_write', 'output', 'reasoning', 'cost')
from .features import FEATURES, feature_name, treatment_pair_verified


def measured(records):
    values, coverage = {}, {}
    for name in DIMENSIONS:
        present = [row.get('incremental', {}).get(name) for row in records]
        known = [value for value in present if isinstance(value, int) and not isinstance(value, bool)]
        values[name] = sum(known) if known else None
        coverage[name] = {'known': len(known), 'expected': len(records)}
    return {'known_sums': values, 'coverage': coverage,
            'cost_qualities': sorted({row.get('cost_quality', 'unknown') for row in records}),
            'currencies': sorted({row['currency'] for row in records if row.get('currency')}),
            'models': sorted({row['model'] for row in records if row.get('model')}),
            'versions': sorted({row['provider_version'] for row in records if row.get('provider_version')}),
            'receipt_count': len(records),
            'complete_successful_receipts': bool(records) and all(row.get('status') == 'recorded' and row.get('outcome') == 'success' for row in records),
            'invocation_elapsed_ms': sum(row.get('elapsed_ms') or 0 for row in records)
                if records and all(row.get('elapsed_ms') is not None for row in records) else None}


def compare(arms):
    pairs = []
    keys = sorted({(arm['tool'], arm['case']) for arm in arms})
    for tool, case in keys:
        group = {arm['arm']: arm for arm in arms if (arm['tool'], arm['case']) == (tool, case)}
        first = next(iter(group.values()))
        names = FEATURES[feature_name(first)]['arms']
        baseline, candidate = (group.get(name) for name in names)
        config_key = 'config_sha256' if feature_name(first) == 'session-reuse' else 'comparison_config_sha256'
        eligible = bool(baseline and candidate and all(arm.get('accepted') for arm in [baseline, candidate])
                        and baseline['metrics']['receipt_count'] == candidate['metrics']['receipt_count'] == 2
                        and baseline['metrics']['complete_successful_receipts'] and candidate['metrics']['complete_successful_receipts']
                        and baseline['metrics']['models'] == candidate['metrics']['models']
                        and baseline['metrics']['models']
                        and all(baseline.get(key) and baseline[key] == candidate.get(key)
                                for key in ['starting_revision', config_key, 'profile_sha256'])
                        and baseline['metrics']['versions'] == candidate['metrics']['versions']
                        and baseline['metrics']['versions']
                        and baseline.get('session_behavior_verified') and candidate.get('session_behavior_verified')
                        and treatment_pair_verified(baseline, candidate))
        gains = {}
        if eligible:
            for name in ['execution_elapsed_ms']:
                av, bv = baseline.get(name), candidate.get(name)
                gains[name] = round(100 * (av - bv) / av, 2) if av and bv is not None else None
            av, bv = baseline['metrics']['invocation_elapsed_ms'], candidate['metrics']['invocation_elapsed_ms']
            gains['invocation_elapsed_ms'] = round(100 * (av - bv) / av, 2) if av and bv is not None else None
            for name in DIMENSIONS:
                a, b = baseline['metrics'], candidate['metrics']
                complete = all(item['coverage'][name]['known'] == 2 for item in [a, b])
                if name == 'cost':
                    complete = complete and a['cost_qualities'] == b['cost_qualities'] == ['exact'] and len(a['currencies']) == 1 and a['currencies'] == b['currencies']
                av, bv = a['known_sums'][name], b['known_sums'][name]
                gains[name] = round(100 * (av - bv) / av, 2) if complete and av and bv is not None else None
        pairs.append({'tool': tool, 'case': case, 'feature': feature_name(first), 'eligible': eligible,
                      'reduction_percent': gains, 'conclusion': 'pilot only; no statistical saving claim'})
    return pairs


def markdown(report):
    lines = [f"# O2 {report.get('feature', 'session-reuse')} pilot", '', f"Mode: {report['mode']}. Maximum model calls: {report['maximum_model_calls']}.",
             '', 'This measures the shared MACC runner, not coordinator scheduling or all O2 features.',
             'Positive reductions mean lower consumption. Missing fields are unknown; partial sums are not total consumption.',
             'Reported provider cost estimates are retained but cannot establish exact financial savings.', '',
             '| Tool | Task | Arm | Accepted | Wall time (ms) | Uncached input | Cached read | Cached write | Output |',
             '|---|---|---|---|---:|---:|---:|---:|---:|']
    for arm in report['arms']:
        values = arm.get('metrics', {}).get('known_sums', {})
        def show(name):
            value = values.get(name)
            return 'unknown' if value is None else str(value)
        lines.append(f"| {arm['tool']} | {arm['case']} | {arm['arm']} | {arm.get('accepted', False)} | {arm.get('execution_elapsed_ms', 'unknown')} | {show('input_uncached')} | {show('cached_read')} | {show('cached_write')} | {show('output')} |")
        if arm.get('error'):
            lines.append(f"\nFailure ({arm['tool']}/{arm['arm']}): {arm['error']}\n")
    lines += ['', '## Paired comparisons', '', '```json', __import__('json').dumps(report.get('pairs', []), indent=2), '```',
              '', 'Full provenance, failures and normalized receipts: report.json and command logs.',
              'Command history: commands.json; procedure.sh references retained fixtures and is not an idempotent retry script.']
    return '\n'.join(lines) + '\n'
