"""Local code-only Graphify launcher. No assistant registration or cloud backend."""
import os
from pathlib import Path
import runpy
import sys

root = Path.cwd().resolve()
# Graphify otherwise re-execs without the launcher to pin its hash seed.
os.environ["PYTHONHASHSEED"] = "0"
sys.dont_write_bytecode = True
sys.pycache_prefix = str(root / "cache" / "python")


def audit(event, args):
    if event.startswith("socket.") or event in ("os.system", "os.posix_spawn", "os.exec", "os.fork", "os.forkpty"):
        raise PermissionError("MACC code-only provider: network/command disabled")
    if event == "subprocess.Popen":
        raise PermissionError("MACC code-only provider: subprocess disabled")
    if event == "open":
        path, mode, flags = args
        writing = (mode and any(c in mode for c in "wax+")) or (
            flags and flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC)
        )
        if writing and isinstance(path, (str, bytes, os.PathLike)):
            resolved = Path(os.fsdecode(path)).resolve()
            if not resolved.is_relative_to(root):
                raise PermissionError("MACC provider write outside owned workspace")


sys.addaudithook(audit)
sys.argv = ["graphify", *sys.argv[1:]]
runpy.run_module("graphify", run_name="__main__")
