#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./scripts/bump-version.sh <X.Y.Z>
  make release-bump VERSION=<X.Y.Z>

Description:
  Deterministically bumps the MACC version across:
  - Cargo.toml ([workspace.package].version)
  - adapters/Cargo.toml ([workspace.package].version)
  - web/package.json & web/package-lock.json
  - web/src/pages/About.tsx (fallback version)
  - CHANGELOG.md (promotes [Unreleased] to [X.Y.Z] - YYYY-MM-DD if not already present)
  - Cargo.lock & adapters/Cargo.lock
EOF
}

if [[ $# -lt 1 || -z "${1:-}" ]]; then
  echo "Error: missing target version (e.g. 0.3.0)" >&2
  usage >&2
  exit 1
fi

case "$1" in
  -h|--help)
    usage
    exit 0
    ;;
esac

RAW_VERSION="$1"
VERSION="${RAW_VERSION#v}"

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "Error: invalid SemVer version '$RAW_VERSION' (expected X.Y.Z)" >&2
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

TODAY="${RELEASE_DATE:-$(date -u +%Y-%m-%d)}"

python3 - "$VERSION" "$TODAY" <<'PY'
import json
import pathlib
import re
import sys

version = sys.argv[1]
today = sys.argv[2]

def update_cargo_workspace_version(path_str: str) -> None:
    p = pathlib.Path(path_str)
    content = p.read_text(encoding="utf-8")
    pattern = re.compile(r'(\[workspace\.package\]\s*\n(?:[^\[]*\n)*?version\s*=\s*)"[^"]+"')
    new_content, count = pattern.subn(rf'\1"{version}"', content, count=1)
    if count != 1:
        raise SystemExit(f"Error: could not update [workspace.package].version in {path_str}")
    p.write_text(new_content, encoding="utf-8")

update_cargo_workspace_version("Cargo.toml")
update_cargo_workspace_version("adapters/Cargo.toml")

pkg_path = pathlib.Path("web/package.json")
pkg_data = json.loads(pkg_path.read_text(encoding="utf-8"))
pkg_data["version"] = version
pkg_path.write_text(json.dumps(pkg_data, indent=2) + "\n", encoding="utf-8")

lock_path = pathlib.Path("web/package-lock.json")
if lock_path.exists():
    lock_data = json.loads(lock_path.read_text(encoding="utf-8"))
    lock_data["version"] = version
    if isinstance(lock_data.get("packages"), dict) and "" in lock_data["packages"]:
        lock_data["packages"][""]["version"] = version
    lock_path.write_text(json.dumps(lock_data, indent=2) + "\n", encoding="utf-8")

about_path = pathlib.Path("web/src/pages/About.tsx")
if about_path.exists():
    about_content = about_path.read_text(encoding="utf-8")
    about_content = re.sub(
        r"(const version = health\?\.version \?\? ')[^']+(';\n)",
        rf"\g<1>{version}\2",
        about_content,
        count=1,
    )
    about_path.write_text(about_content, encoding="utf-8")

changelog_path = pathlib.Path("CHANGELOG.md")
changelog = changelog_path.read_text(encoding="utf-8")
section_re = re.compile(rf"^## \[{re.escape(version)}\]", re.M)
if not section_re.search(changelog):
    unreleased_re = re.compile(r"^## \[Unreleased\]\s*\n+", re.M)
    if not unreleased_re.search(changelog):
        raise SystemExit("Error: CHANGELOG.md is missing '## [Unreleased]' section")
    replacement = f"## [Unreleased]\n\n## [{version}] - {today}\n\n"
    changelog = unreleased_re.sub(replacement, changelog, count=1)
    changelog_path.write_text(changelog, encoding="utf-8")
PY

echo "Updating Cargo.lock and adapters/Cargo.lock..."
cargo update -w --offline >/dev/null 2>&1 || cargo metadata --format-version 1 >/dev/null
(cd adapters && (cargo update -w --offline >/dev/null 2>&1 || cargo metadata --format-version 1 >/dev/null))

make check-version TAG_VERSION="v${VERSION}"
echo "Successfully bumped MACC to v${VERSION} (${TODAY})."
