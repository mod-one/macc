#!/usr/bin/env bash
# Exercise the real installer, Git, Cargo and npm without a remote dependency.
set -euo pipefail

installer="$(cd "$(dirname "$0")/.." && pwd -P)/install.sh"
fixture="$(mktemp -d)"
trap 'rm -rf -- "$fixture"' EXIT
export TMPDIR="$fixture/tmp with ' spaces"
export CARGO_NET_OFFLINE=true CARGO_TARGET_DIR=target npm_config_offline=true
mkdir -p "$TMPDIR"

origin="$fixture/origin"
mkdir -p "$origin"/{cli/src,scripts,web/asset}
git -C "$origin" init -qb main
cp "$installer" "$origin/scripts/install.sh"
cp "$(dirname "$installer")/uninstall.sh" "$origin/scripts/"
cat > "$origin/Cargo.toml" <<'EOF'
[workspace]
members = ["cli"]
resolver = "2"
EOF
cat > "$origin/cli/Cargo.toml" <<'EOF'
[package]
name = "macc"
version = "0.0.1"
edition = "2021"
EOF
cat > "$origin/cli/src/main.rs" <<'EOF'
fn main() {
    #[cfg(not(debug_assertions))]
    assert_eq!(include_str!("../../web/dist.txt"), "web ready");
    println!("fixture {}", env!("MACC_BUILD_SHA"));
}
EOF
cat > "$origin/web/package.json" <<'EOF'
{"name":"fixture","version":"1.0.0","devDependencies":{"asset":"file:asset"},"scripts":{"build":"node build.cjs"}}
EOF
echo '{"name":"asset","version":"1.0.0","main":"index.cjs"}' > "$origin/web/asset/package.json"
echo 'module.exports = "web ready";' > "$origin/web/asset/index.cjs"
echo 'require("node:fs").writeFileSync("dist.txt", require("asset"));' > "$origin/web/build.cjs"
(cd "$origin/web" && npm install --package-lock-only --ignore-scripts --no-audit --no-fund)
git -C "$origin" add .
git -C "$origin" -c user.name=Test -c user.email=test@example.invalid commit -qm fixture
sha="$(git -C "$origin" rev-parse --short HEAD)"
ref="$(git -C "$origin" rev-parse HEAD)"

cd "$fixture"
check_install() {
  [[ -x "$1/macc-uninstall" && "$("$1/macc" --version)" == "fixture $sha" ]]
}
no_temp_clone() {
  ! compgen -G "$TMPDIR/macc-install.*" >/dev/null
}

cp "$installer" standalone.sh
bash standalone.sh --repo "$origin" --ref main --no-path --prefix default-bin
check_install default-bin
no_temp_clone

# stdin install covers commit fallback, retained source and diagnostic stream.
bash -s -- --repo "$origin" --ref "$ref" --keep-src --no-path --prefix kept-bin \
  < standalone.sh > stdout.log 2> stderr.log
check_install kept-bin
kept="$(sed -n 's/^Using fetched source (kept): //p' stderr.log)"
[[ -d "$kept/.git" && "$(git -C "$kept" rev-parse HEAD)" == "$ref" ]]
if grep -Fq "$kept" stdout.log; then exit 1; fi

git clone -q "$origin" checkout
[[ ! -e checkout/web/node_modules ]]
# An invalid fetch URL ensures the installer uses the local working copy.
bash checkout/scripts/install.sh --release --repo "$fixture/missing" --no-path --prefix release-bin
check_install release-bin
[[ -d checkout/web/node_modules ]]
git -C checkout worktree add -q --detach "$fixture/linked"
[[ -f linked/.git ]]
bash linked/scripts/install.sh --repo "$fixture/missing" --no-path --prefix linked-bin
check_install linked-bin
[[ -x linked/target/debug/macc ]]

# Cleanup also runs on a failed checkout; a previous --keep-src clone is untouched.
if bash standalone.sh --repo "$origin" --ref invalid-ref --no-path --prefix failed-bin; then exit 1; fi
shopt -s nullglob
remaining=("$TMPDIR"/macc-install.*)
[[ ${#remaining[@]} == 1 && "${remaining[0]}/repo" == "$kept" && ! -e failed-bin ]]
echo 'Installer smoke tests passed.'
