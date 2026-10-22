"""Experiment contracts, independent of provider CLI syntax."""
from . import memory_cases, navigation_cases, output_cases

FEATURES = {
    'session-reuse': {
        'arms': ('fresh', 'reuse'), 'default_case': 'slug-list',
        'cases': ('slug-list', 'bounded-values'),
        'baseline': 'Close the available logical session between steps',
        'candidate': 'Reuse the available logical/native session',
    },
    'memory-context': {
        'arms': ('no-push', 'push'), 'default_case': 'reference-code',
        'cases': tuple(memory_cases.CASES),
        'baseline': 'Approved source-backed memory exists; automatic context injection disabled',
        'candidate': 'Same approved corpus; enable only context_engine.enabled',
    },
    'native-navigation': {
        'arms': ('literal', 'indexed'), 'default_case': 'charge-policy',
        'cases': tuple(navigation_cases.CASES),
        'baseline': 'Public native literal references supplied as navigation evidence',
        'candidate': 'Parsed native definition supplied as navigation evidence; index preparation measured separately',
    },
    'output-reduction': {
        'arms': ('full', 'reduced'), 'default_case': 'slug-diagnostics',
        'cases': tuple(output_cases.CASES),
        'baseline': 'Full real Python test diagnostics supplied to the agent',
        'candidate': 'Same test workflow; public macc o2 reduce test supplies bounded diagnostics with raw evidence',
    },
    'memory-mcp': {
        'arms': ('push', 'pull'), 'default_case': 'reference-code',
        'cases': tuple(memory_cases.CASES),
        'baseline': 'Approved policy automatically pushed at both steps; MCP available but unused',
        'candidate': 'Agent retrieves the same approved policy through a real scoped stdio MCP client, then reuses it',
    },
    'graphify-navigation': {
        'arms': ('native', 'provider'), 'default_case': 'charge-policy',
        'cases': tuple(navigation_cases.CASES),
        'baseline': 'Native literal references supplied once before editing',
        'candidate': 'Real managed Graphify traversal supplied once before editing; preparation measured separately',
    },
    'serena-navigation': {
        'arms': ('native', 'provider'), 'default_case': 'charge-policy',
        'cases': tuple(navigation_cases.CASES),
        'baseline': 'Native parsed definition supplied once before editing',
        'candidate': 'Real managed Serena Python/Jedi definition supplied once before editing',
    },
}


def feature_name(arm):
    return arm.get('feature', 'session-reuse')


def treatment_pair_verified(baseline, candidate):
    if feature_name(baseline) != feature_name(candidate):
        return False
    if feature_name(baseline) == 'session-reuse':
        return True
    if feature_name(baseline) in ['graphify-navigation', 'serena-navigation']:
        a, b = baseline.get('provider_experiment', {}), candidate.get('provider_experiment', {})
        return (a.get('verified') is True and b.get('verified') is True
                and a.get('candidate') is False and b.get('candidate') is True
                and a.get('provider') == b.get('provider')
                and a.get('corpus_sha256') and a['corpus_sha256'] == b.get('corpus_sha256'))
    if feature_name(baseline) == 'memory-mcp':
        a, b = baseline.get('mcp_experiment', {}), candidate.get('mcp_experiment', {})
        return (a.get('verified') is True and b.get('verified') is True
                and a.get('mode') == 'push' and b.get('mode') == 'pull'
                and a.get('client_sha256') and a['client_sha256'] == b.get('client_sha256')
                and a.get('corpus_sha256') and a['corpus_sha256'] == b.get('corpus_sha256'))
    if feature_name(baseline) == 'output-reduction':
        a, b = baseline.get('output_experiment', {}), candidate.get('output_experiment', {})
        return (a.get('verified') is True and b.get('verified') is True
                and a.get('enabled') is False and b.get('enabled') is True
                and a.get('corpus_sha256') and a['corpus_sha256'] == b.get('corpus_sha256'))
    if feature_name(baseline) == 'native-navigation':
        a, b = baseline.get('navigation_experiment', {}), candidate.get('navigation_experiment', {})
        return (a.get('verified') is True and b.get('verified') is True
                and a.get('indexed') is False and b.get('indexed') is True
                and a.get('corpus_sha256') and a['corpus_sha256'] == b.get('corpus_sha256'))
    a, b = baseline.get('memory_experiment', {}), candidate.get('memory_experiment', {})
    return (a.get('verified') is True and b.get('verified') is True
            and a.get('enabled') is False and b.get('enabled') is True
            and a.get('corpus_sha256') and a['corpus_sha256'] == b.get('corpus_sha256'))
