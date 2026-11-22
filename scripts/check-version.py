"""Check release versions from the repository root."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib


def workspace_version(manifest):
    with manifest.open("rb") as stream:
        return tomllib.load(stream)["workspace"]["package"]["version"]


def check_versions(tag):
    root = Path.cwd()
    workspace = Path("Cargo.toml")
    expected = tag.removeprefix("v") if tag else workspace_version(workspace)
    errors = set()

    def check(path, actual):
        if actual != expected:
            errors.add(f"{path}: version {actual} does not match expected {expected!r}")

    for manifest in (workspace, Path("adapters/Cargo.toml")):
        check(manifest, workspace_version(manifest))
        # Cargo resolves membership and inherited versions in each workspace.
        metadata = json.loads(subprocess.check_output([
            "cargo", "metadata", "--no-deps", "--offline", "--format-version", "1",
            "--manifest-path", str(manifest),
        ]))
        for package in metadata["packages"]:
            check(Path(package["manifest_path"]).relative_to(root), package["version"])

    web = Path("web/package.json")
    check(web, json.loads(web.read_text(encoding="utf-8"))["version"])
    changelog = Path("CHANGELOG.md").read_text(encoding="utf-8")
    if not re.search(rf"^## \[{re.escape(expected)}\](?:\s|$)", changelog, re.MULTILINE):
        errors.add(f"CHANGELOG.md: missing ## [{expected}]")
    if errors:
        sys.exit("\n".join(sorted(errors)))
    print(f"Version check OK: {expected}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tag", nargs="?", help="release tag or version (default: workspace version)")
    check_versions(parser.parse_args().tag)
