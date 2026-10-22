"""Authoritative source policies, equally available to both memory experiment arms."""
CASES = {
    'reference-code': {
        'source': 'def reference_code(namespace, number):\n    raise NotImplementedError\n\ndef format_references(rows):\n    raise NotImplementedError\n',
        'steps': [
            'Implement reference_code(namespace, number) according to the documented project reference policy under docs/. Use its formatting and validation rules.',
            'Implement format_references(rows) according to the same reference policy under docs/. Preserve reference_code behavior and input data.',
        ],
        'policy': '# Project reference code policy\n\n'
                  'reference_code(namespace, number) accepts two strings. Strip whitespace from both. '
                  'Normalize namespace to lowercase; its stripped value must be nonempty and contain ASCII letters only. '
                  'The stripped number must be nonempty and contain ASCII digits only. '
                  'Raise ValueError for an invalid namespace or number. Preserve leading zeroes in number. '
                  'Join namespace and number with the literal separator ::. Example: " Case ", " 007 " produces "case::007".\n\n'
                  'format_references(rows) takes a list of (namespace, number) pairs and returns a new list using reference_code. '
                  'Preserve order and duplicates; never mutate rows. Propagate ValueError from invalid rows. An empty list returns an empty list.\n',
        'memory_title': 'Reference code namespace number formatting policy',
        'checks': [
            [('reference_code', [' Case ', ' 007 '], 'case::007'), ('reference_code', ['a', '0'], 'a::0'),
             ('reference_code', ['', '1'], {'raises': 'ValueError'}), ('reference_code', ['a-b', '1'], {'raises': 'ValueError'}),
             ('reference_code', ['a', '-1'], {'raises': 'ValueError'}), ('reference_code', ['a', ''], {'raises': 'ValueError'}),
             ('reference_code', ['é', '1'], {'raises': 'ValueError'}), ('reference_code', ['a', '١'], {'raises': 'ValueError'})],
            [('format_references', [[('Case', '007'), ('a', '1'), ('Case', '007')]], ['case::007', 'a::1', 'case::007']),
             ('format_references', [[]], []), ('format_references', [[('a', '-1')]], {'raises': 'ValueError'})],
        ],
    },
    'display-label': {
        'source': 'def display_label(given, family):\n    raise NotImplementedError\n\ndef format_people(rows):\n    raise NotImplementedError\n',
        'steps': [
            'Implement display_label(given, family) according to the documented project display label policy under docs/. Apply its empty-name and whitespace conventions.',
            'Implement format_people(rows) according to the same display label policy under docs/. Preserve display_label behavior and input data.',
        ],
        'policy': '# Project display label policy\n\n'
                  'display_label(given, family) accepts two strings. Collapse each string\'s whitespace using single spaces and trim it. '
                  'Preserve the given name\'s case and uppercase the family name. '
                  'When both parts are nonempty, join them using the literal separator " / ". '
                  'When only one part is nonempty, return that part alone. When both are empty, return the literal "-". '
                  'Example: " Ada  Mae ", " van  Rossum " produces "Ada Mae / VAN ROSSUM".\n\n'
                  'format_people(rows) takes a list of (given, family) pairs and returns a new list using display_label. '
                  'Preserve order and duplicates; never mutate rows. An empty list returns an empty list.\n',
        'memory_title': 'Display label people names whitespace policy',
        'checks': [
            [('display_label', [' Ada  Mae ', ' van\tRossum '], 'Ada Mae / VAN ROSSUM'),
             ('display_label', [' Ada ', ' '], 'Ada'), ('display_label', ['', ' Lovelace '], 'LOVELACE'),
             ('display_label', [' \t ', '\n'], '-')],
            [('format_people', [[('Ada', 'Lovelace'), ('', ''), ('Ada', 'Lovelace')]], ['Ada / LOVELACE', '-', 'Ada / LOVELACE']),
             ('format_people', [[]], [])],
        ],
    },
}
