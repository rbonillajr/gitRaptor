#!/bin/sh
# Stages the engine binaries as Tauri sidecars (TS-DSK-002 T008, ADR-GRP-014 DEP-DSK-5):
#
#   sh tools/desktop/stage-sidecars.sh <target-triple> <dir-with-binaries>
#
# Copies `raptor` and `raptor-mcp` from <dir> to apps/desktop/src-tauri/binaries/raptor-<triple> and
# raptor-mcp-<triple> (the names `externalBin` expects) with mode 755. Fails when one is missing.
# Run from the repo root.
set -eu

fail() { printf 'stage-sidecars: error: %s\n' "$*" >&2; exit 1; }

[ $# -eq 2 ] || fail "usage: stage-sidecars.sh <target-triple> <dir-with-binaries>"
triple="$1"; src="$2"
case "$triple" in *[!A-Za-z0-9_-]* | "") fail "bad target triple: $triple" ;; esac

for b in raptor raptor-mcp; do
  [ -f "$src/$b" ] || fail "missing $src/$b"
done

dest="apps/desktop/src-tauri/binaries"
mkdir -p "$dest"
for b in raptor raptor-mcp; do
  cp "$src/$b" "$dest/$b-$triple"
  chmod 755 "$dest/$b-$triple"
done
ls -l "$dest"
