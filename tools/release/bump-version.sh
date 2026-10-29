#!/bin/sh
# Sets the workspace version for a release (ADR-GRP-014, 2026-10-28 amendment).
#
#   sh tools/release/bump-version.sh 0.1.0-alpha.2
#
# Edits `version` under [workspace.package] in the root Cargo.toml and lets cargo refresh the
# entries of the workspace crates in Cargo.lock, offline. Fails if anything else in Cargo.lock
# changed (a dependency must never move in a release-prep PR). Run it on a branch cut from
# origin/main; the tag goes on the merged commit, never on this branch.
set -eu

fail() { printf 'bump-version: error: %s\n' "$*" >&2; exit 1; }

[ $# -eq 1 ] || fail "usage: bump-version.sh <version>   (for example 0.1.0-alpha.2)"
new="$1"
printf '%s' "$new" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$' || fail "invalid version: $new"

root=$(git rev-parse --show-toplevel) || fail "not inside a Git repository"
cd "$root"
[ -z "$(git status --porcelain Cargo.toml Cargo.lock)" ] || fail "Cargo.toml or Cargo.lock has local changes"

current=$(awk '/^\[workspace.package\]/ { in_ws = 1; next } /^\[/ { in_ws = 0 } in_ws && /^version *=/ { gsub(/[" ]/, "", $3); print $3; exit }' Cargo.toml)
[ -n "$current" ] || fail "no version under [workspace.package] in Cargo.toml"
[ "$current" != "$new" ] || fail "the workspace is already at $new"

awk -v new="$new" '
  /^\[workspace.package\]/ { in_ws = 1 }
  /^\[/ && !/^\[workspace.package\]/ { in_ws = 0 }
  in_ws && /^version *=/ && !done { print "version = \"" new "\""; done = 1; next }
  { print }
' Cargo.toml > Cargo.toml.new && mv Cargo.toml.new Cargo.toml

cargo update --workspace --offline >/dev/null 2>&1 || { git checkout -- Cargo.toml Cargo.lock; fail "cargo update --workspace failed; nothing was changed"; }

# Only lines that say `version = "<current>"` may differ in Cargo.lock.
stray=$(git diff -U0 Cargo.lock | grep -E '^[+-][^+-]' | grep -Ev "^[+-]version = \"($current|$new)\"$" || true)
if [ -n "$stray" ]; then
  git checkout -- Cargo.toml Cargo.lock
  fail "Cargo.lock changed beyond the workspace versions; reverted. Offending lines:
$stray"
fi
cargo metadata --no-deps --format-version 1 --locked >/dev/null || fail "Cargo.lock is not consistent with Cargo.toml"

printf 'bump-version: %s -> %s\n' "$current" "$new"
git diff --stat Cargo.toml Cargo.lock
