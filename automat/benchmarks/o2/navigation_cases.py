"""Frozen small navigation tasks with literal distractors and one real definition."""


def case(symbol, module, operation, checks):
    files = {'policies/__init__.py': '', f'policies/{module}.py':
             f'class {symbol}:\n    @staticmethod\n    def evaluate(value):\n        {operation}\n'}
    for number in range(24):
        files[f'notes/example_{number:02d}.py'] = (
            f'# Historical example mentioning {symbol}; not its definition.\n'
            f'EXAMPLE = "{symbol}.evaluate({number})"\n' if number < 6
            else f'UNRELATED_EXAMPLE = {number}\n')
    return {'source': 'def evaluate(value):\n    raise NotImplementedError\n\ndef evaluate_all(values):\n    raise NotImplementedError\n',
            'files': files, 'symbol': symbol, 'definition_path': f'policies/{module}.py',
            'delegate': f'policies.{module}',
            'steps': [f'Implement evaluate(value) by delegating to the existing authoritative {symbol}.evaluate(value). Locate its definition; do not duplicate the policy.',
                      'Implement evaluate_all(values): return a new list using evaluate, preserve order and duplicates, never mutate values. Keep evaluate unchanged.'],
            'checks': [checks, [('evaluate_all', [[0, 2, 2, 10]], [checks[0][2], checks[1][2], checks[1][2], checks[2][2]]), ('evaluate_all', [[]], [])]]}


CASES = {
    'charge-policy': case('ChargePolicy', 'billing', 'return max(0, value) * 3 + 7',
                          [('evaluate', [0], 7), ('evaluate', [2], 13), ('evaluate', [10], 37), ('evaluate', [-5], 7)]),
    'retention-policy': case('RetentionPolicy', 'retention', 'return min(30, max(0, value)) + 2',
                             [('evaluate', [0], 2), ('evaluate', [2], 4), ('evaluate', [10], 12), ('evaluate', [40], 32)]),
}
