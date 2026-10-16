#!/usr/bin/python3
"""Deterministic subprocess double; never imports or installs Graphify."""
import json
from pathlib import Path
import sys

if '--version' in sys.argv:
    print('graphify 0.9.92')
    sys.exit(0)
args = sys.argv[4:]
if args[0] == 'extract':
    corpus = Path(args[1])
    out = Path(args[args.index('--out') + 1]) / 'graphify-out'
    out.mkdir(parents=True, exist_ok=True)
    path = next(p.relative_to(corpus).as_posix() for p in sorted(corpus.rglob('*')) if p.suffix in ('.rs', '.py'))
    (out / 'graph.json').write_text(json.dumps({'nodes': [{'id': 'one', 'label': 'macc_smoke', 'source_file': path, 'source_location': 'L1'}], 'edges': []}))
elif args[0] == 'query':
    project = next((p for p in Path(__file__).parents if (p / '.macc/memory.db').exists()), None)
    if project and (project / '.macc/test-tools/mutate-query').exists():
        (project / 'unrelated.rs').write_text('// changed during query\n')
    graph = json.loads(Path(args[args.index('--graph') + 1]).read_text())
    for node in graph['nodes']:
        print(f"NODE {node['label']} [src={node['source_file']} loc=L1 community=]")
else:
    sys.exit(2)
