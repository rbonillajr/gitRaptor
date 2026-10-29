#!/bin/sh
# Picks what a GitHub Release attaches (ADR-GRP-014, 2026-10-28 amendment).
#
#   sh tools/release/select-assets.sh <version> <dist-dir> <out-dir>
#
# An early-access alpha (`*-alpha.*`) ships macOS only: the 2 apple-darwin archives with their
# .sha256, the SBOMs, install.sh and a SHA256SUMS that lists only what is attached. Any other
# version attaches everything in <dist-dir>. The build still covers the six targets.
set -eu

fail() { printf 'select-assets: error: %s\n' "$*" >&2; exit 1; }

[ $# -eq 3 ] || fail "usage: select-assets.sh <version> <dist-dir> <out-dir>"
version="$1"; dist="$2"; out="$3"
[ -f "$dist/SHA256SUMS" ] || fail "no SHA256SUMS in $dist"
mkdir -p "$out"

case "$version" in
  *-alpha.*)
    n=0
    for f in "$dist/raptor-$version-"*-apple-darwin.tar.gz*; do
      [ -f "$f" ] || continue
      cp "$f" "$out/"
      case "$f" in *.sha256) ;; *) n=$((n + 1)) ;; esac
    done
    [ "$n" -eq 2 ] || fail "expected 2 macOS archives for $version, found $n"
    for f in "$dist/raptor-$version-sbom-"*.cdx.json; do
      [ -f "$f" ] || continue
      cp "$f" "$out/"
    done
    cp "$dist/install.sh" "$out/"
    # `grep -F` on the exact names: the version is dotted text, not a pattern.
    : > "$out/SHA256SUMS"
    for f in "$out"/raptor-*; do
      case "$f" in *.sha256) continue ;; esac
      grep -F "$(basename "$f")" "$dist/SHA256SUMS" >> "$out/SHA256SUMS" \
        || fail "$(basename "$f") is not in SHA256SUMS"
    done
    [ "$(wc -l < "$out/SHA256SUMS" | tr -d ' ')" -ge 4 ] || fail "SHA256SUMS lists fewer files than expected"
    ;;
  *) cp "$dist"/* "$out/" ;;
esac
ls -l "$out"
