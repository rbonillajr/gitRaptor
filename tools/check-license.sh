#!/usr/bin/env bash
# Every crate and package declares the project license (ADR-GRP-014 § 6, NFR-11).
#
#   tools/check-license.sh
#
# Checks the root LICENSE, every package of every Cargo workspace in the repo (product
# workspace and standalone spikes, through `cargo metadata`) and every tracked package.json.
# Needs git, cargo and jq. Read-only.
set -euo pipefail

expected="FSL-1.1-ALv2"
# Official text (getsentry/fsl.software, FSL-1.1-ALv2.template.md at 85f3fc7) with the notice
# filled in: "Copyright 2026 Rene Bonilla".
license_sha256="b15e18a7da7abf99cef235645f890df590aea81b5cd7963f39866d3a686dc18e"
cd "$(git rev-parse --show-toplevel)"
fail=0

crates=0
packages=0

if command -v sha256sum >/dev/null; then sha=(sha256sum); else sha=(shasum -a 256); fi
actual=$("${sha[@]}" LICENSE 2>/dev/null || true)
if [ "${actual%% *}" != "$license_sha256" ]; then
  echo "::error file=LICENSE::LICENSE is missing or is not the official FSL-1.1-ALv2 text"
  fail=1
fi

# Workspace roots: every tracked Cargo.toml with a [workspace] table.
while IFS= read -r manifest; do
  grep -q '^\[workspace\]' "$manifest" || continue
  # Captured first: a failing `cargo metadata` aborts here instead of yielding no crates.
  meta=$(cargo metadata --no-deps --format-version 1 --manifest-path "$manifest")
  while IFS=$'\t' read -r name license path; do
    crates=$((crates + 1))
    if [ "$license" != "$expected" ]; then
      echo "::error file=${path#"$PWD/"}::crate $name declares license '$license', expected '$expected'"
      fail=1
    fi
  done < <(jq -r '.packages[] | [.name, (.license // "none"), .manifest_path] | @tsv' <<<"$meta")
done < <(git ls-files '*Cargo.toml' 'Cargo.toml')

while IFS= read -r pkg; do
  packages=$((packages + 1))
  license=$(jq -r '.license // "none"' "$pkg")
  if [ "$license" != "$expected" ]; then
    echo "::error file=$pkg::package $(jq -r .name "$pkg") declares license '$license', expected '$expected'"
    fail=1
  fi
done < <(git ls-files '*package.json' 'package.json')

if [ "$crates" -eq 0 ] || [ "$packages" -eq 0 ]; then
  echo "::error::checked $crates crates and $packages packages: the check found nothing to check"
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "all $crates crates and $packages packages declare $expected"
