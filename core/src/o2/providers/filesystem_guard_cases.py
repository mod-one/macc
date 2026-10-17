# Appended after the actual emitted launcher guard in a disposable test process.
outside = Path.cwd().parent / "outside"
fd = os.open(outside, os.O_RDONLY)
read_fd = os.open(outside / "victim", os.O_RDONLY)
inside = Path.cwd() / "inside"
inside.write_text("allowed")
checks = [
    lambda: (outside / "newdir").mkdir(),
    lambda: (outside / "victim").unlink(),
    lambda: (outside / "empty").rmdir(),
    lambda: (outside / "victim").rename("stolen"),
    lambda: inside.rename(outside / "replaced"),
    lambda: os.link(outside / "victim", "linked"),
    lambda: os.link(inside, outside / "linked"),
    lambda: os.symlink(outside / "victim", "symlink"),
    lambda: os.chmod(outside / "victim", 0o600),
    lambda: os.utime(outside / "victim", None),
    lambda: os.truncate(outside / "victim", 0),
    lambda: os.fchmod(read_fd, 0o600),
    lambda: os.ftruncate(read_fd, 0),
    lambda: os.mkdir("newdir", dir_fd=fd),
    lambda: os.unlink("victim", dir_fd=fd),
    lambda: os.rename("victim", "stolen", src_dir_fd=fd),
    lambda: os.link("victim", "linked", src_dir_fd=fd),
    lambda: os.link("inside", "linked", dst_dir_fd=fd),
    lambda: os.open("victim", os.O_WRONLY | os.O_TRUNC, dir_fd=fd),
    lambda: (Path("escape") / "newdir").mkdir(),
    lambda: (Path("escape") / "victim").unlink(),
    lambda: open(bytes(outside / "victim"), "w"),
    lambda: os.mkfifo(outside / "pipe"),
    lambda: os.chdir(outside),
]
for check in checks:
    try:
        check()
    except PermissionError:
        continue
    raise AssertionError("Filesystem mutation escaped: " + str(check))
os.close(fd)
os.close(read_fd)
Path("allowed-dir").mkdir()
with open("allowed-dir/file", "w") as stream:
    stream.write("allowed")
os.link("allowed-dir/file", "allowed-dir/link")
os.rename("allowed-dir/link", "allowed-dir/renamed")
os.chmod("allowed-dir/file", 0o600)
os.utime("allowed-dir/file", None)
os.truncate("allowed-dir/file", 2)
with os.fdopen(os.open("allowed-dir/fdfile", os.O_CREAT | os.O_WRONLY, 0o600), "w") as stream:
    stream.write("owned fd")
Path("allowed-dir/file").unlink()
Path("allowed-dir/renamed").unlink()
Path("allowed-dir/fdfile").unlink()
Path("allowed-dir").rmdir()
inside.unlink()
print("filesystem-boundary-ok")
