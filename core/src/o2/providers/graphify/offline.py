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
    filesystem_audit(root, event, args)  # Shared guard prepended by runtime.rs.



install_filesystem_guard(root)
sys.addaudithook(audit)
sys.argv = ["graphify", *sys.argv[1:]]
runpy.run_module("graphify", run_name="__main__")
