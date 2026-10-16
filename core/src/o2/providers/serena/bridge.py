"""MACC's read-only bridge to pinned Serena SolidLSP and Python/Jedi.

Only curated source copies enter the LSP workspace. Both Python processes use
this launcher; no Serena agent, MCP server, project hooks or edits are exposed.
"""
import json
import logging
import os
from pathlib import Path
import sys
import shlex

is_server = sys.argv[-1] == "--server"
work = Path.cwd().resolve()
launcher = str(Path(__file__).resolve())
server_argv = [sys.executable, "-B", "-I", launcher, "--server"]
sys.dont_write_bytecode = True
sys.pycache_prefix = str(work / "cache" / "python")


def audit(event, args):
    if event in ("socket.connect", "socket.connect_ex", "socket.bind", "socket.getaddrinfo",
                 "os.system", "os.posix_spawn", "os.exec", "os.fork", "os.forkpty"):
        raise PermissionError("MACC Serena: network and arbitrary commands disabled")
    if event == "subprocess.Popen":
        if list(args[1]) != ["/bin/sh", "-c", shlex.join(server_argv)] or args[0] != "/bin/sh" or is_server:
            raise PermissionError("MACC Serena: only the pinned Jedi stdio child is allowed")
    if event == "open":
        path, mode, flags = args
        writing = (mode and any(c in mode for c in "wax+")) or (
            flags and flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC))
        if writing and isinstance(path, (str, bytes, os.PathLike)):
            if not Path(os.fsdecode(path)).resolve().is_relative_to(work):
                raise PermissionError("MACC Serena: write outside owned scratch")


logging.basicConfig(level=logging.WARNING)
sys.addaudithook(audit)
if is_server:
    # Keep the stdio child tied to its launcher even after an abrupt parent exit.
    import ctypes
    parent_pid = os.getppid()
    if ctypes.CDLL(None, use_errno=True).prctl(1, 9) != 0 or parent_pid == 1 or os.getppid() != parent_pid:
        raise RuntimeError("Cannot establish Jedi parent-death protection")
    # Jedi must parse source, never import/execute project modules.
    import jedi
    jedi.settings.allow_unsafe_interpreter_executions = False
    from jedi.api.environment import InterpreterEnvironment
    # Avoid Jedi's unguarded helper interpreter. The pinned same-process
    # environment performs inference with unsafe interpreter executions disabled.
    jedi.Project.get_environment = lambda self: InterpreterEnvironment()
    from jedi_language_server.server import SERVER
    sys.argv = ["jedi-language-server"]
    # The pinned pygls synchronous stdio loop avoids an async reader stall.
    SERVER._start_io_sync()
else:
    def run_query():
        from importlib.metadata import version
        from solidlsp.language_servers.jedi_server import JediServer
        from solidlsp.ls_config import LanguageServerConfig, LanguageServerId
        from solidlsp.lsp_protocol_handler.server import ProcessLaunchInfo
        from solidlsp.settings import SolidLSPSettings

        if version("serena-agent") != "2.0.0" or version("jedi-language-server") != "0.46.0":
            raise RuntimeError("Serena/Jedi version mismatch")
        request = json.loads((work / "request.json").read_text())
        source = work / "sources"

        class ManagedJedi(JediServer):
            def _get_process_launch_info(self):
                return ProcessLaunchInfo(cmd=server_argv, cwd=str(work))

        settings = SolidLSPSettings(solidlsp_dir=str(work / "cache" / "solidlsp"),
                                   project_data_path=str(work / "cache" / "project"))
        server = ManagedJedi(LanguageServerConfig(ls_id=LanguageServerId.PYTHON_JEDI, start_independent_lsp_process=False), str(source), settings)
        server.set_request_timeout(request["timeout"])
        hits = []
        matches = []

        def visit(symbols, prefix, path):
            for symbol in symbols:
                name = prefix + symbol["name"]
                if name == request["text"].lstrip("/") or ("/" not in request["text"] and symbol["name"] == request["text"]):
                    matches.append((path, symbol["selectionRange"]["start"], name))
                visit(symbol.get("children", []), name + "/", path)

        with server.start_server_context():
            for path in sorted(request["files"]):
                if request.get("path") and path != request["path"]:
                    continue
                visit(server.request_document_symbols(path).root_symbols, "", path)
            if request["kind"] == "references" and len(matches) > 1:
                raise ValueError("Ambiguous symbol; supply its definition path and qualified name")
            for path, pos, name in matches:
                if request["kind"] == "symbol_definition":
                    hits.append({"path": path, "line": pos["line"] + 1, "name": name})
                else:
                    for ref in server.request_references(path, pos["line"], pos["character"]):
                        rel = ref.get("relativePath")
                        if rel in request["files"]:
                            hits.append({"path": rel, "line": ref["range"]["start"]["line"] + 1, "name": name})
        # Explicit success with no matches/references is authoritative, not literal search.
        unique = {(h["path"], h["line"], h["name"]): h for h in hits}
        response = {"schema": 1, "language": "python", "backend": "jedi", "matches": len(matches),
                    "hits": [unique[k] for k in sorted(unique)][:request["limit"]]}
        (work / "response.json").write_text(json.dumps(response))

    try:
        run_query()
    except Exception as exc:
        (work / "response.json").write_text(json.dumps({"error": str(exc)[:500]}))
