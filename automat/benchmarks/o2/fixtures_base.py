"""Small base implementation contracts shared by isolated benchmark features."""
BASE_CASES = {
    'slug-list': {
        'source': 'def slug(text):\n    raise NotImplementedError\n\ndef unique_slugs(values):\n    raise NotImplementedError\n',
        'steps': [
            'Implement slug(text): strip outer whitespace, lowercase ASCII text, split on whitespace and join words with a single hyphen. Empty/whitespace input returns an empty string.',
            'Implement unique_slugs(values) using slug: normalize each string, discard empty results and duplicates, and preserve first-seen order. Keep slug behavior unchanged.',
        ],
        'checks': [
            [('slug', ['  Hello   WORLD '], 'hello-world'), ('slug', [' \t '], ''), ('slug', ['A\tB'], 'a-b')],
            [('unique_slugs', [[' A B ', '', 'a b', 'C', 'c', 'D E']], ['a-b', 'c', 'd-e']), ('unique_slugs', [[]], [])],
        ],
    },
    'bounded-values': {
        'source': 'def clamp(value, lower, upper):\n    raise NotImplementedError\n\ndef clamp_all(values, lower, upper):\n    raise NotImplementedError\n',
        'steps': [
            'Implement clamp(value, lower, upper) for integers: bound the result inclusively; raise ValueError when lower exceeds upper.',
            'Implement clamp_all(values, lower, upper): return a new list using clamp, never mutate values, and raise ValueError for inverted bounds even when values is empty. Keep clamp behavior unchanged.',
        ],
        'checks': [
            [('clamp', [-2, 0, 9], 0), ('clamp', [12, 0, 9], 9), ('clamp', [4, 0, 9], 4), ('clamp', [1, 3, 2], {'raises': 'ValueError'})],
            [('clamp_all', [[-2, 4, 12], 0, 9], [0, 4, 9]), ('clamp_all', [[], 3, 2], {'raises': 'ValueError'})],
        ],
    },
}
