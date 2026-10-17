#!/usr/bin/env bash
# Exercise real CLI launches and canonical ownership with deterministic fixtures.
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo test -p macc-core --test daemon_ownership_integration --locked
cargo test -p macc-cli --test supervisor_launch --test supervisor_intervention --locked
