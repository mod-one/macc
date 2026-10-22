"""Small independent contracts; checks live outside the agent's writable project."""
import runpy
import sys
from pathlib import Path
try:
    from .fixtures_base import BASE_CASES
    from .output_cases import CASES as OUTPUT_CASES
    from .memory_cases import CASES as MEMORY_CASES
    from .navigation_cases import CASES as NAVIGATION_CASES
except ImportError:
    # Independent verifier runs as a standalone isolated subprocess.
    from fixtures_base import BASE_CASES
    from output_cases import CASES as OUTPUT_CASES
    from memory_cases import CASES as MEMORY_CASES
    from navigation_cases import CASES as NAVIGATION_CASES

CASES = dict(BASE_CASES)
CASES.update(MEMORY_CASES)
CASES.update(NAVIGATION_CASES)
CASES.update(OUTPUT_CASES)


def prompt(case, step):
    return ("Work only on solution.py in the current directory. Do not commit, install dependencies, "
            "access the network or inspect .macc/log or other conversations. Use Python standard library only; use python3 for local checks. "
            "Complete only the requested step. Keep your final explanation under 60 words.\n"
            + CASES[case]['steps'][step]
            + '\nEmit any checkpoint acknowledgement requested by MACC, then end with exactly:\n'
            'MACC_TASK_RESULT: success_with_changes\n')


def verify(path, case, step):
    # Fixture imports are scoped to this isolated verifier, never the benchmark host.
    previous = sys.path[:]
    try:
        sys.path.insert(0, str(Path(path).resolve().parent))
        namespace = runpy.run_path(str(path))
    finally:
        sys.path[:] = previous
    checked = 0
    for checks in CASES[case]['checks'][:step + 1]:
        for name, args, expected in checks:
            original = repr(args)
            try:
                value = namespace[name](*args)
            except ValueError:
                if expected != {'raises': 'ValueError'}:
                    raise
            else:
                if expected == {'raises': 'ValueError'}:
                    raise AssertionError(f'{name}: expected ValueError')
                if value != expected:
                    raise AssertionError(f'{name}: expected {expected!r}, got {value!r}')
            if repr(args) != original:
                raise AssertionError(f'{name} mutated its input')
            checked += 1
    if 'delegate' in CASES[case]:
        policy = getattr(sys.modules[CASES[case]['delegate']], CASES[case]['symbol'])
        original_evaluate = policy.evaluate
        try:
            policy.evaluate = staticmethod(lambda value: 1000 + value)
            if namespace['evaluate'](123) != 1123:
                raise AssertionError('Implementation must delegate to the authoritative policy')
            checked += 1
        finally:
            policy.evaluate = staticmethod(original_evaluate)
    return checked
