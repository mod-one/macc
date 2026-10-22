"""Opt-in short paired experiments, one O2 feature at a time."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

from .execution import exercise, sha
from .fixtures import CASES
from .metrics import compare, markdown
from .transport import Commands
from .features import FEATURES
from . import retention

SOURCE = Path(__file__).resolve().parents[3]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--feature', choices=sorted(FEATURES), default='session-reuse')
    parser.add_argument('--tool', action='append', help='Registered adapter; default: codex and claude')
    parser.add_argument('--case', action='append', choices=sorted(CASES))
    parser.add_argument('--settings', type=Path, required=True, help='Reviewed JSON mapping tool IDs to canonical tools.config settings')
    parser.add_argument('--tier', default='standard')
    parser.add_argument('--first-arm', choices=sorted({arm for feature in FEATURES.values() for arm in feature['arms']}), help='First tool/case order; subsequent pairs alternate')
    parser.add_argument('--tool-spec', type=Path, action='append', default=[], help='Optional full ToolSpec JSON for a custom registered adapter')
    parser.add_argument('--macc', type=Path, default=SOURCE / 'target/debug/macc')
    parser.add_argument('--output', type=Path, required=True, help='New private directory; never overwritten')
    parser.add_argument('--archive-root', type=Path, help='Keep a complete immutable evidence copy under this directory, including failed runs')
    parser.add_argument('--execute', action='store_true', help='Allow the planned AI calls; default is a plan without probes')
    parser.add_argument('--prepare-only', action='store_true', help='Managed-provider lifecycle/calibration only; allows pinned downloads but no AI calls')
    parser.add_argument('--timeout', type=int, default=120, help='Per-call timeout; no billed retries')
    args = parser.parse_args(argv)
    feature = FEATURES[args.feature]
    if args.prepare_only and (args.execute or args.feature not in ['graphify-navigation', 'serena-navigation']):
        parser.error('--prepare-only requires a managed-provider feature and excludes --execute')
    tools, cases = args.tool or ['codex', 'claude'], args.case or [feature['default_case']]
    args.first_arm = args.first_arm or feature['arms'][0]
    if args.first_arm not in feature['arms'] or any(case not in feature['cases'] for case in cases):
        parser.error('The selected cases/first arm do not belong to this feature')
    if len(set(tools)) != len(tools) or len(set(cases)) != len(cases) or len(tools) > 2:
        parser.error('Choose up to two distinct tools and each case once')
    if any(not re.fullmatch(r'[a-z][a-z0-9-]{0,63}', tool) for tool in tools):
        parser.error('Invalid tool identity')
    if not 10 <= args.timeout <= 300:
        parser.error('Timeout must be 10..300 seconds')
    raw = args.settings.read_bytes()
    if len(raw) > 65536:
        parser.error('Settings exceed 64 KiB')
    settings = json.loads(raw)
    specs = {}
    for path in args.tool_spec:
        spec = json.loads(path.read_bytes())
        if spec.get('id') not in tools or spec['id'] in specs:
            parser.error('Each custom ToolSpec must name one distinct selected tool')
        specs[spec['id']] = path.resolve()
    for tool in tools:
        tier = settings.get(tool, {}).get('model_tiers', {}).get(args.tier, {})
        if not all(isinstance(tier.get(key), str) and tier[key].strip() and '{' not in tier[key] for key in ['model', 'effort']):
            parser.error(f'{tool} needs explicit model_tiers.{args.tier}.model and effort')
    args.output = args.output.resolve()
    args.macc = args.macc.resolve()
    args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
    archive = retention.reserve(args.output, args.archive_root)
    (args.output / 'settings.json').write_bytes(raw)
    os.chmod(args.output / 'settings.json', 0o600)
    snapshot = args.output / 'harness'
    snapshot.mkdir()
    for path in Path(__file__).parent.glob('*.py'):
        (snapshot / path.name).write_bytes(path.read_bytes())
    active = args.execute or args.prepare_only
    report = {'schema_version': 1, 'feature': args.feature, 'mode': 'prepare' if args.prepare_only else ('execute' if args.execute else 'plan'),
              'status': 'running' if active else 'planned',
              'archive_path': str(archive) if archive else None,
              'maximum_model_calls': 0 if args.prepare_only else 4 * len(tools) * len(cases), 'tools': tools, 'cases': cases,
              'settings_sha256': hashlib.sha256(raw).hexdigest(), 'arms': [], 'pairs': [],
              'custom_specs': {tool: sha(path) for tool, path in specs.items()},
              'harness_files': {str(path.relative_to(SOURCE)): sha(path) for path in sorted(Path(__file__).parent.glob('*.py'))},
              'source_revision': subprocess.check_output(['git', '-C', str(SOURCE), 'rev-parse', 'HEAD'], text=True).strip(),
              'protocol': {'calls_per_arm': 0 if args.prepare_only else 2, 'retries': 0, 'session_policy_identical': True,
                           'baseline': feature['baseline'],
                           'candidate': feature['candidate'],
                           'quality': 'Independent behavioral assertions and write-scope checks',
                           'order': 'Alternate first arm across tool/case pairs', 'first_arm': args.first_arm,
                           'cache': 'No artificial provider-cache flush; report cached usage separately',
                           'conclusion': 'Descriptive pilot; not a statistical saving claim'}}
    commands = Commands(args.output)
    try:
        if active:
            report['macc_sha256'] = sha(args.macc)
            report['macc_version'] = commands.run([args.macc, '--version'], SOURCE).strip()
            failed_tools = set()
            for index, (tool, case) in enumerate((tool, case) for tool in tools for case in cases):
                if tool in failed_tools:
                    continue
                order = list(feature['arms'])
                if args.first_arm != order[0]:
                    order.reverse()
                for arm in (order if index % 2 == 0 else list(reversed(order))):
                    print(f'Running {tool} / {case} / {arm}', flush=True)
                    result = exercise(commands, args.macc, args.output, tool, settings[tool], case, arm, args.timeout, args.tier, specs.get(tool), feature=args.feature, prepare_only=args.prepare_only)
                    report['arms'].append(result)
                    write_report(args.output, report)
                    if result.get('interrupted'):
                        report['pairs'] = compare(report['arms'])
                        raise RuntimeError('Operator interrupted the experiment; no remaining tool was started')
                    if not result['accepted']:
                        # Stop this tool's pair on any failure/quota/timeout; retain all evidence.
                        failed_tools.add(tool)
                        break
        report['pairs'] = [] if args.prepare_only else compare(report['arms'])
        report['status'] = ('failed' if any(not arm['accepted'] for arm in report['arms']) else ('prepared' if args.prepare_only else 'measured')) if active else 'planned'
    except (OSError, ValueError, RuntimeError, KeyboardInterrupt) as error:
        report['status'] = 'failed'
        report['error'] = str(error)[:1000] or 'Operator interrupted the experiment'
    finally:
        commands.save()
        write_report(args.output, report)
        retention.preserve(args.output, archive)
    print(args.output / 'analysis.md')
    return int(report['status'] == 'failed')


def write_report(output, report):
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    (output / 'analysis.md').write_text(markdown(report))
