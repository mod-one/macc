"""Bounded command execution and retained replay evidence; no provider syntax."""
import json
import os
from pathlib import Path
import shlex
import signal
import subprocess
import time


class ExperimentInterrupted(RuntimeError):
    """Stop the whole experiment after retaining the interrupted arm."""


def terminate_group(process):
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    # The wrapper can exit before its runner finishes native-process cleanup.
    # Allow the existing runner trap/watchdog to run before killing the group.
    deadline = time.monotonic() + 3
    while time.monotonic() < deadline:
        process.poll()
        try:
            os.killpg(process.pid, 0)
        except ProcessLookupError:
            break
        time.sleep(0.05)
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait(timeout=3)


class Commands:
    def __init__(self, output):
        self.output = Path(output)
        self.entries = []

    def run(self, argv, cwd, env=None, timeout=30, input_text=None):
        argv = [str(item) for item in argv]
        index = len(self.entries)
        log = self.output / f'command-{index:03d}.log'
        entry = {'argv': argv, 'cwd': str(cwd), 'env': env or {}, 'timeout_seconds': timeout,
                 'stdin': input_text, 'log': str(log), 'status': 'pending'}
        self.entries.append(entry)
        self.save()
        started = time.monotonic()
        try:
            with log.open('xb') as stream:
                process = subprocess.Popen(argv, cwd=cwd, env=dict(os.environ, **(env or {})),
                                           stdin=subprocess.PIPE, stdout=stream, stderr=subprocess.STDOUT,
                                           start_new_session=True)
                try:
                    process.communicate(input_text.encode() if input_text else None, timeout=timeout)
                except (subprocess.TimeoutExpired, KeyboardInterrupt) as error:
                    terminate_group(process)
                    entry['exit_code'] = process.returncode
                    if isinstance(error, KeyboardInterrupt):
                        entry['status'] = 'interrupted'
                        raise ExperimentInterrupted(f'Experiment interrupted; retained evidence: {log}') from error
                    entry['status'] = 'timeout'
                    raise RuntimeError(f'Command timed out; retained evidence: {log}')
                entry['exit_code'] = process.returncode
                entry['status'] = 'ok' if process.returncode == 0 else 'failed'
            if log.stat().st_size > 16 * 1024 * 1024:
                raise RuntimeError(f'Command output exceeds evidence limit: {log}')
            text = log.read_text(errors='replace')
            if process.returncode:
                raise RuntimeError(f'Command exited {process.returncode}; inspect {log}')
            return text
        except OSError:
            entry['status'] = 'launch_failed'
            raise
        finally:
            entry['elapsed_ms'] = round((time.monotonic() - started) * 1000)
            self.save()

    def save(self):
        (self.output / 'commands.json').write_text(json.dumps(self.entries, indent=2) + '\n')
        lines = ['#!/bin/sh', '# Replay only against the retained disposable fixtures.', 'set -eu']
        for entry in self.entries:
            prefix = 'env ' + ' '.join(shlex.quote(f'{key}={value}') for key, value in entry['env'].items())
            command = shlex.join(entry['argv'])
            if entry['env']:
                command = prefix + ' ' + command
            if entry['stdin'] is not None:
                command = 'printf %s ' + shlex.quote(entry['stdin']) + ' | ' + command
            lines.append('(cd ' + shlex.quote(entry['cwd']) + ' && ' + command + ')')
        (self.output / 'procedure.sh').write_text('\n'.join(lines) + '\n')
