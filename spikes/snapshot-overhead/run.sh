#!/usr/bin/env bash
# SPIKE-TMC-001 — reproduce every measurement with a single command.
#
#   spikes/snapshot-overhead/run.sh            # reference repo (profile M), full bench
#   spikes/snapshot-overhead/run.sh S --iters 20 --week 50   # quick smoke run
#
# Everything (generated repo, worktrees, stores) lives in a scratch directory outside any
# Git repository (SPIKE_ROOT, or a fresh mktemp dir). Nothing is written to this repository.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
profile="${1:-M}"
shift || true
root="${SPIKE_ROOT:-$(mktemp -d -t spike-tmc-001)}"
cargo build --release --quiet --manifest-path "$here/Cargo.toml"
"$here/target/release/snapshot-overhead" bench --root "$root" --profile "$profile" "$@"
echo "results: $root/results-$profile.md"
