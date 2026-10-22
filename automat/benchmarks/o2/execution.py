"""One experiment arm, using public MACC commands and the registered adapter runner."""
import hashlib
import json
from pathlib import Path
import re
import sys
import time

from .fixtures import CASES, prompt
from .metrics import measured
from . import memory_experiment, navigation_experiment, output_experiment, memory_mcp, provider_navigation
from .transport import ExperimentInterrupted


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def scope_snapshot(commands, worktree):
    files = commands.run(['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], worktree)
    return {name: sha(worktree / name) if (worktree / name).is_file() else 'missing'
            for name in files.split('\0') if name and name != 'solution.py'}


def exercise(commands, macc, output, tool, settings, case, arm, timeout, tier, tool_spec=None, feature='session-reuse', prepare_only=False):
    root = output / f'{tool}-{case}-{arm}'
    root.mkdir()
    task = 'BENCH-' + case.upper()
    result = {'tool': tool, 'case': case, 'arm': arm, 'feature': feature, 'fixture': str(root),
              'accepted': False, 'calls': [], 'status': 'failed', 'task_id': task}
    started = time.monotonic()
    execution_started = None
    cli = [str(macc), '--cwd', str(root)]
    def invoke(*args, **kwargs):
        return commands.run(cli + list(args), root, **kwargs)
    try:
        commands.run(['git', 'init', '-b', 'main'], root)
        commands.run(['git', 'config', 'user.email', 'benchmark@example.invalid'], root)
        commands.run(['git', 'config', 'user.name', 'MACC benchmark'], root)
        commands.run(['git', 'config', 'commit.gpgsign', 'false'], root)
        (root / '.gitignore').write_text('.macc/\nworktree.prd.json\n__pycache__/\n')
        (root / 'solution.py').write_text(CASES[case]['source'])
        for name, content in CASES[case].get('files', {}).items():
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        if feature in ['memory-context', 'memory-mcp']:
            (root / 'docs').mkdir()
            (root / 'docs/policy.md').write_text(CASES[case]['policy'])
        (root / 'AGENTS.md').write_text('Modify only solution.py. No dependencies, network access or Git commits.\n')
        commands.run(['git', 'add', '.'], root)
        commands.run(['git', 'commit', '-m', 'Deterministic O2 benchmark fixture'], root,
                     env={'GIT_AUTHOR_DATE': '2000-01-01T00:00:00Z', 'GIT_COMMITTER_DATE': '2000-01-01T00:00:00Z'})
        result['starting_revision'] = commands.run(['git', 'rev-parse', 'HEAD'], root).strip()
        invoke('init', '--fresh', '--no-restore-prompt')
        if tool_spec is not None:
            directory = root / '.macc/tools.d'
            directory.mkdir(exist_ok=True)
            (directory / f'{tool}.tool.yaml').write_bytes(Path(tool_spec).read_bytes())
        config = {'tools': {'enabled': [tool], 'config': {tool: settings}},
                  'settings': {'offline': True},
                  'session_management': {'usage': {'capture_mode': 1}, 'checkpoints': {'rotate_after_uses': 20}},
                  'memory': {'enabled': False}, 'context_engine': {'enabled': False},
                  'adaptive_compaction': {'enabled': False}, 'code_intelligence': {'enabled': False}}
        if feature in ['memory-context', 'memory-mcp']:
            config['memory']['enabled'] = True
            config['context_engine'].update(output_reduction=False, push_memory_limit_tokens=600)
        if feature == 'memory-mcp':
            config['memory'].update(mcp_enabled=True, agent_proposals_allowed=False, mcp_output_tokens=1000)
        if feature == 'native-navigation':
            config['code_intelligence'].update(enabled=True, native_map_enabled=arm == 'indexed')
        if feature == 'output-reduction':
            config['context_engine'].update(output_reduction=arm == 'reduced', pull_limit_tokens=2000, max_output_bytes=65536)
        if feature in provider_navigation.PROVIDERS:
            provider_navigation.configure(config, feature, arm == 'provider')
        (root / '.macc/macc.yaml').write_text(json.dumps(config, indent=2))
        result['config_sha256'] = sha(root / '.macc/macc.yaml')
        invoke('worktree', 'create', 'pilot', '--tool', tool, '--base', 'main', '--skip-apply')
        wt = root / '.macc/worktree/pilot'
        spec_path = wt / '.macc/tool.json'
        spec = json.loads(spec_path.read_text())
        runner = Path(spec['performer']['runner'])
        if not runner.is_file():
            raise RuntimeError('Resolved adapter runner is missing')
        tier_config = spec.get('model_tiers', {}).get(tier, {})
        if tier_config.get('model') != settings['model_tiers'][tier]['model'] or tier_config.get('effort') != settings['model_tiers'][tier]['effort']:
            raise RuntimeError('Resolved model/effort differs from the reviewed settings')
        # Absolute fixture runner paths vary by arm; the executable content must not.
        comparable = json.loads(json.dumps(spec))
        comparable['performer']['runner'] = sha(runner)
        result['profile_sha256'] = hashlib.sha256(json.dumps(comparable, sort_keys=True).encode()).hexdigest()
        result['model_effort'] = tier_config
        result['spec_sha256'] = sha(spec_path)
        result['runner_sha256'] = sha(runner)
        # Synthetic runtime contract, not a coordinator production PRD/queue.
        (wt / 'worktree.prd.json').write_text(json.dumps({'tasks': [{
            'id': task, 'title': case, 'description': ' '.join(CASES[case]['steps']),
            'acceptance_criteria': CASES[case]['steps'], 'allowed_paths': ['solution.py'],
        }]}))
        result['contract_sha256'] = sha(wt / 'worktree.prd.json')
        migration = json.loads(invoke('session-store', 'migrate'))
        invoke('session-store', 'migrate', '--apply', '--expected-sha256', migration['source_sha256'])
        if feature == 'memory-context':
            state, comparable_config = memory_experiment.prepare(invoke, root, config, case, task, arm == 'push')
            result['memory_experiment'] = state
            result['comparison_config_sha256'] = comparable_config
            result['config_sha256'] = sha(root / '.macc/macc.yaml')
        if feature in provider_navigation.PROVIDERS:
            state, comparable_config = provider_navigation.prepare(invoke, root, wt, config, case, feature, arm == 'provider')
            result['provider_experiment'] = state
            result['comparison_config_sha256'] = comparable_config
            if prepare_only:
                result.update(accepted=True, status='prepared', preparation_elapsed_ms=round((time.monotonic() - started) * 1000))
                return result
        if feature == 'native-navigation':
            state, comparable_config = navigation_experiment.prepare(invoke, root, wt, config, case, arm == 'indexed')
            result['navigation_experiment'] = state
            result['comparison_config_sha256'] = comparable_config
        if feature == 'output-reduction':
            state, comparable_config = output_experiment.prepare(invoke, config, case, arm == 'reduced')
            result['output_experiment'] = state
            result['comparison_config_sha256'] = comparable_config
        if feature == 'memory-mcp':
            state, comparable_config = memory_mcp.prepare(invoke, root, wt, config, case, task, arm == 'push')
            result['mcp_experiment'] = state
            result['comparison_config_sha256'] = comparable_config
            result['config_sha256'] = sha(root / '.macc/macc.yaml')
        scope_before = scope_snapshot(commands, wt)
        execution_started = time.monotonic()
        result['preparation_elapsed_ms'] = round((execution_started - started) * 1000)
        snapshots = []
        for step in range(2):
            prompt_file = wt / f'.macc/prompt-{step + 1}.txt'
            text = prompt(case, step)
            if feature == 'native-navigation':
                text += navigation_experiment.evidence(invoke, wt, case, state)
            if feature == 'output-reduction':
                text += output_experiment.evidence(commands, invoke, root, wt, case, step, state)
            if feature == 'memory-mcp':
                text += memory_mcp.instruction(state, step)
            if feature in provider_navigation.PROVIDERS:
                text += provider_navigation.evidence(invoke, wt, case, state, step)
            prompt_file.write_text(text)
            env = {'MACC_SESSION_SERVICE_BIN': str(macc), 'MACC_MODEL_TIER': tier,
                   'MACC_MODEL_ROUTING_MODE': 'auto', 'MACC_SESSION_ROLE': 'performer',
                   'MACC_SESSION_PHASE': 'dev'}
            call = {'step': step + 1, 'command_index': len(commands.entries)}
            result['calls'].append(call)
            text = invoke('worktree', 'exec', 'pilot', '--', 'bash', str(runner),
                          '--repo', str(root), '--worktree', str(wt), '--task-id', task,
                          '--tool-json', str(spec_path), '--prompt-file', str(prompt_file),
                          '--max-attempts', '1', env=env, timeout=timeout)
            if not re.search(r'^MACC_TASK_RESULT:\s*success_with_changes\s*$', text, re.M):
                raise RuntimeError('Missing successful completion marker; no automatic retry')
            # Run independent trusted assertions outside the model's writable fixture.
            commands.run([sys.executable, '-I', str(Path(__file__).with_name('verify.py')),
                          str(wt / 'solution.py'), case, str(step)], root)
            if scope_snapshot(commands, wt) != scope_before:
                raise RuntimeError('Agent changed files outside the allowed implementation')
            if sha(spec_path) != result['spec_sha256'] or sha(root / '.macc/macc.yaml') != result['config_sha256']:
                raise RuntimeError('Execution profile or benchmark policy changed during the task')
            if sha(wt / 'worktree.prd.json') != result['contract_sha256']:
                raise RuntimeError('Mandatory task contract changed during the task')
            head = commands.run(['git', 'rev-parse', 'HEAD'], wt).strip()
            if head != result['starting_revision']:
                raise RuntimeError('Agent committed or changed starting revision')
            view = json.loads(invoke('session-store', 'status'))
            sessions = [s for s in view['sessions'] if s['task_id'] == task and s['tool_id'] == tool and s['status'] == 'available']
            if len(sessions) != 1:
                raise RuntimeError('Expected one available session after each invocation')
            snapshots.append(sessions[0])
            if feature == 'session-reuse' and step == 0 and arm == 'fresh':
                invoke('session-store', 'close', sessions[0]['id'])
        same = snapshots[0]['id'] == snapshots[1]['id']
        native = all(s.get('provider_session_id') for s in snapshots)
        expect_reuse = feature != 'session-reuse' or arm == 'reuse'
        result['session_behavior_verified'] = bool(native and (same if expect_reuse else not same))
        result['sessions'] = snapshots
        if feature == 'native-navigation':
            navigation_experiment.inspect(state)
        if feature == 'output-reduction':
            output_experiment.inspect(state)
        if feature in provider_navigation.PROVIDERS:
            result['execution_elapsed_ms'] = round((time.monotonic() - execution_started) * 1000)
            provider_navigation.inspect(invoke, wt, case, state)
        result['accepted'] = result['session_behavior_verified']
        result['status'] = 'measured' if result['accepted'] else 'session_unverified'
    except (OSError, ValueError, RuntimeError, KeyError) as error:
        result['error'] = str(error)[:1000]
        if isinstance(error, ExperimentInterrupted):
            result.update(status='interrupted', interrupted=True)
    finally:
        if execution_started is not None:
            result.setdefault('execution_elapsed_ms', round((time.monotonic() - execution_started) * 1000))
        try:
            usage = json.loads(invoke('session-store', 'usage'))
            result['usage'] = usage
            result['metrics'] = measured(usage.get('records', []))
            if feature == 'memory-context' and result.get('accepted'):
                try:
                    memory_experiment.inspect(invoke, result['memory_experiment'], usage.get('records', []))
                except (OSError, ValueError, RuntimeError, KeyError) as error:
                    result.update(accepted=False, status='treatment_unverified', error=str(error))
            if feature == 'memory-mcp' and result.get('accepted'):
                try:
                    memory_mcp.inspect(invoke, wt, result['mcp_experiment'], usage.get('records', []))
                except (OSError, ValueError, RuntimeError, KeyError) as error:
                    result.update(accepted=False, status='treatment_unverified', error=str(error))
        except (OSError, ValueError, RuntimeError):
            result['metrics'] = measured([])
        result['total_elapsed_ms'] = round((time.monotonic() - started) * 1000)
    return result
