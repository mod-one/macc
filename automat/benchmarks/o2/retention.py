"""Private evidence archives, including unsuccessful experiments; never replace a run."""
import hashlib
import json
from pathlib import Path
import shutil


def reserve(source, archive_root):
    if archive_root is None:
        return None
    target = Path(archive_root).resolve() / source.name
    if target.is_relative_to(source) or source.is_relative_to(target):
        raise ValueError('Evidence archive and execution directory must not overlap')
    target.mkdir(parents=True, mode=0o700, exist_ok=False)
    return target


def preserve(source, target):
    if target is None:
        return
    if any(target.iterdir()):
        raise FileExistsError('Refusing to replace existing archived evidence')
    shutil.copytree(source, target, dirs_exist_ok=True, symlinks=True)
    files = []
    for path in sorted(target.rglob('*')):
        name = str(path.relative_to(target))
        if path.is_symlink():
            files.append({'path': name, 'symlink': str(path.readlink())})
        elif path.is_file():
            files.append({'path': name, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
    (target / 'archive_manifest.json').write_text(json.dumps({'complete': True, 'files': files}, indent=2) + '\n')
