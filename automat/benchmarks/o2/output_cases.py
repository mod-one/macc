"""Real noisy unittest diagnostics; the AI implements only two tiny functions."""
try:
    from .fixtures_base import BASE_CASES
except ImportError:
    from fixtures_base import BASE_CASES

# Stdlib tests actually execute solution.py. The wrapper keeps the test exit result
# explicit in JSON while returning successfully after collecting its diagnostics.
DIAGNOSTICS = '''import io
import json
import sys
import unittest
import solution

class ContractChecks(unittest.TestCase):
    def test_requested_behavior(self):
        if CASE == 'slug-diagnostics':
            if STEP == 0:
                self.assertEqual(solution.slug('  Hello   WORLD '), 'hello-world')
            else:
                self.assertEqual(solution.unique_slugs([' A B ', '', 'a b', 'C']), ['a-b', 'c'])
        elif STEP == 0:
            self.assertEqual(solution.clamp(-2, 0, 9), 0)
        else:
            self.assertEqual(solution.clamp_all([-2, 4, 12], 0, 9), [0, 4, 9])

class BackgroundChecks(unittest.TestCase):
    pass

def background(self):
    self.assertEqual(2 + 2, 4)

for number in range(200):
    setattr(BackgroundChecks, f'test_existing_behavior_{number:03d}', background)
CASE, STEP = sys.argv[1], int(sys.argv[2])
suite = unittest.TestSuite([unittest.defaultTestLoader.loadTestsFromTestCase(BackgroundChecks),
                           unittest.defaultTestLoader.loadTestsFromTestCase(ContractChecks)])
stream = io.StringIO()
result = unittest.TextTestRunner(stream=stream, verbosity=2).run(suite)
print(json.dumps({'exit_code': 0 if result.wasSuccessful() else 1,
                  'tests_run': result.testsRun, 'output': stream.getvalue()}))
'''

CASES = {}
for name, base, source in [
    ('slug-diagnostics', 'slug-list', 'def slug(text):\n    return text.strip().replace(" ", "_")\n\ndef unique_slugs(values):\n    raise NotImplementedError\n'),
    ('clamp-diagnostics', 'bounded-values', 'def clamp(value, lower, upper):\n    return value\n\ndef clamp_all(values, lower, upper):\n    raise NotImplementedError\n'),
]:
    CASES[name] = dict(BASE_CASES[base], source=source, files={'diagnostics.py': DIAGNOSTICS})
