#!/bin/sh
# Test for select-assets.sh with fake assets in a temporary folder.
# shellcheck disable=SC2012 # file names here are fixed ASCII
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(mktemp -d); trap 'rm -rf "$root"' EXIT INT TERM
V=0.1.0-alpha.3
d="$root/dist"; mkdir -p "$d"
for t in x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin; do
  echo data > "$d/raptor-$V-$t.tar.gz"; echo sum > "$d/raptor-$V-$t.tar.gz.sha256"
done
for t in x86_64-pc-windows-msvc aarch64-pc-windows-msvc; do echo data > "$d/raptor-$V-$t.zip"; done
for c in cli mcp; do echo sbom > "$d/raptor-$V-sbom-$c.cdx.json"; done
echo script > "$d/install.sh"; echo ps > "$d/install.ps1"
for f in "$d"/raptor-*; do case "$f" in *.sha256) ;; *) printf 'abc  %s\n' "$(basename "$f")" ;; esac; done | sort > "$d/SHA256SUMS"

sh "$here/select-assets.sh" "$V" "$d" "$root/out" >/dev/null
names=$(LC_ALL=C ls "$root/out" | tr '\n' ' ')
want="SHA256SUMS install.sh raptor-$V-aarch64-apple-darwin.tar.gz raptor-$V-aarch64-apple-darwin.tar.gz.sha256 raptor-$V-sbom-cli.cdx.json raptor-$V-sbom-mcp.cdx.json raptor-$V-x86_64-apple-darwin.tar.gz raptor-$V-x86_64-apple-darwin.tar.gz.sha256 "
[ "$names" = "$want" ] || { echo "FAIL alpha asset set: $names"; exit 1; }
[ "$(wc -l < "$root/out/SHA256SUMS" | tr -d ' ')" = 4 ] || { echo "FAIL SHA256SUMS lines"; exit 1; }
grep -q 'linux\|windows' "$root/out/SHA256SUMS" && { echo "FAIL non-macOS in SHA256SUMS"; exit 1; }
echo "ok   alpha ships macOS only"

# A stable version attaches everything.
sh "$here/select-assets.sh" 0.1.0 "$d" "$root/all" >/dev/null
[ "$(ls "$d" | wc -l)" = "$(ls "$root/all" | wc -l)" ] || { echo "FAIL stable set"; exit 1; }
echo "ok   stable ships everything"

# Fewer than 2 macOS archives aborts.
rm "$d/raptor-$V-x86_64-apple-darwin.tar.gz"
if sh "$here/select-assets.sh" "$V" "$d" "$root/bad" >/dev/null 2>&1; then echo "FAIL missing macOS archive"; exit 1; fi
echo "ok   missing macOS archive aborts"
