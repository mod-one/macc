#!/usr/bin/env bash
# Exercise release validation against real tags without changing the source checkout.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
fixture="$(mktemp -d)"
trap 'rm -rf -- "$fixture"' EXIT
git clone --quiet --depth 1 --no-local --no-tags "$root" "$fixture/repo"
cd "$fixture/repo"

version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')"
git tag "v$version-mismatch"
check_tag() {
  python3 "$root/scripts/check-version.py" "$(git describe --tags --exact-match HEAD)"
}
if check_tag > "$fixture/output" 2>&1; then
  echo "Release check accepted the wrong Git tag" >&2
  exit 1
fi
grep -Fx "Cargo.toml: version $version does not match expected '$version-mismatch'" "$fixture/output"

git tag -d "v$version-mismatch" >/dev/null
git tag "v$version"
check_tag

# A separately pinned member must not slip through when the workspace matches.
sed -E 's/^version(\.workspace)?[[:space:]]*=.*/version = "0.0.0-regression"/' cli/Cargo.toml > "$fixture/manifest"
mv "$fixture/manifest" cli/Cargo.toml
if check_tag > "$fixture/output" 2>&1; then
  echo "Release check accepted a mismatched CLI crate" >&2
  exit 1
fi
grep -F "cli/Cargo.toml: version 0.0.0-regression does not match expected '$version'" "$fixture/output"
