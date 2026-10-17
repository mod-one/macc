"""Filesystem audit guard prepended to each managed Python launcher by MACC.

This is defense in depth for trusted pinned dependencies, not an OS sandbox.
Descriptor-relative mutations are rejected: audit events cannot prove fd ownership.
"""
import os
from pathlib import Path


def filesystem_audit(root, event, args):
    def owned(path, dir_fd=None):
        if dir_fd not in (None, -1):
            raise PermissionError("MACC provider: descriptor-relative mutation disabled")
        if isinstance(path, int):
            # open(fd), fchmod and ftruncate do not expose the original path.
            try:
                path = os.readlink(f"/proc/self/fd/{path}")
            except OSError:
                raise PermissionError("MACC provider: unverified filesystem descriptor") from None
        if not isinstance(path, (str, bytes, os.PathLike)):
            raise PermissionError("MACC provider: unverified filesystem path")
        if not Path(os.fsdecode(path)).resolve().is_relative_to(root):
            raise PermissionError("MACC provider mutation outside owned scratch")

    if event == "open":
        path, mode, flags = args
        writing = (mode and any(c in mode for c in "wax+")) or (
            flags and flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC | os.O_APPEND))
        if writing:
            owned(path)
    elif event in ("os.mkdir", "os.chmod", "os.chown", "os.utime"):
        owned(args[0], args[-1])
    elif event in ("os.remove", "os.rmdir"):
        owned(args[0], args[1])
    elif event in ("os.rename", "os.link"):
        owned(args[0], args[2])
        owned(args[1], args[3])
    elif event == "os.symlink":
        # A symlink can grant a later write through a path outside scratch.
        # Providers do not need to create symlinks; reject even dangling targets.
        raise PermissionError("MACC provider: symlink creation disabled")
    elif event in ("os.truncate", "os.setxattr", "os.removexattr"):
        owned(args[0])
    elif event in ("os.chdir", "os.fchdir"):
        # Keep relative path resolution and the workspace boundary stable.
        owned(args[0])


def install_filesystem_guard(root):
    # CPython's `open` audit event omits dir_fd. Reject that form before the
    # event can mistake it for a path relative to cwd. mkfifo/mknod have no
    # equivalent path audit event on the supported CPython runtime.
    original_open = os.open

    def guarded_open(path, flags, mode=0o777, *, dir_fd=None):
        if dir_fd is not None:
            raise PermissionError("MACC provider: descriptor-relative open disabled")
        return original_open(path, flags, mode)

    os.open = guarded_open
    for name in ("mkfifo", "mknod"):
        original = getattr(os, name, None)
        if original is not None:
            def guarded(path, *args, _original=original, **kwargs):
                if kwargs.get("dir_fd") is not None:
                    raise PermissionError("MACC provider: descriptor-relative mutation disabled")
                filesystem_audit(root, "os.mkdir", (path, 0, -1))
                return _original(path, *args, **kwargs)
            setattr(os, name, guarded)
