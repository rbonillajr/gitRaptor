#!/bin/sh
# DSK002-DMG-ASSETS (TS-DSK-002 T008 step 2): stage-sidecars.sh <target-triple> <dir> copies
# `raptor` and `raptor-mcp` to apps/desktop/src-tauri/binaries/raptor-<triple> and
# raptor-mcp-<triple> with mode 755, and fails when one is missing.
# The script is copied into a temporary tree and run from there, so the real repo is never touched.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(mktemp -d); trap 'rm -rf "$root"' EXIT INT TERM
T=aarch64-apple-darwin

[ -f "$here/stage-sidecars.sh" ] || { echo "FAIL tools/desktop/stage-sidecars.sh does not exist"; exit 1; }
mkdir -p "$root/repo/tools/desktop"
cp "$here/stage-sidecars.sh" "$root/repo/tools/desktop/stage-sidecars.sh"
run() { (cd "$root/repo" && sh tools/desktop/stage-sidecars.sh "$@"); }
bin="$root/repo/apps/desktop/src-tauri/binaries"

# Both binaries present, deliberately not executable: the script sets the mode.
src="$root/in"; mkdir -p "$src"
printf 'raptor-body\n' > "$src/raptor"; printf 'mcp-body\n' > "$src/raptor-mcp"
chmod 644 "$src/raptor" "$src/raptor-mcp"
run "$T" "$src" >/dev/null 2>&1 || { echo "FAIL stage-sidecars.sh failed with both binaries"; exit 1; }
for pair in "raptor:raptor-$T" "raptor-mcp:raptor-mcp-$T"; do
  from=${pair%%:*}; to=${pair#*:}
  [ -f "$bin/$to" ] || { echo "FAIL $to was not staged"; exit 1; }
  cmp -s "$src/$from" "$bin/$to" || { echo "FAIL $to differs from $from"; exit 1; }
  mode=$(ls -l "$bin/$to" | cut -c1-10)
  [ "$mode" = "-rwxr-xr-x" ] || { echo "FAIL $to mode is $mode, want 755"; exit 1; }
done
echo "ok   stages both sidecars with mode 755"

# A missing binary fails, whichever it is.
for gone in raptor raptor-mcp; do
  rm -rf "$root/repo/apps" "$root/part"; mkdir -p "$root/part"
  for b in raptor raptor-mcp; do [ "$b" = "$gone" ] || echo x > "$root/part/$b"; done
  if run "$T" "$root/part" >/dev/null 2>&1; then echo "FAIL missing $gone did not fail"; exit 1; fi
done
echo "ok   fails when a binary is missing"

# Wrong usage fails.
if run >/dev/null 2>&1; then echo "FAIL no arguments did not fail"; exit 1; fi
echo "ok   fails without arguments"
