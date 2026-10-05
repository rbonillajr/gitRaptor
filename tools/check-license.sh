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
cd "$(git rev-parse --show-toplevel)"
fail=0

if ! head -n 1 LICENSE 2>/dev/null | grep -q "Functional Source License, Version 1.1, ALv2 Future License"; then
  echo "::error file=LICENSE::LICENSE is missing or is not the FSL-1.1-ALv2 text"
  fail=1
fi

# Workspace roots: every tracked Cargo.toml with a [workspace] table.
while IFS= read -r manifest; do
  grep -q '^\[workspace\]' "$manifest" || continue
  while IFS=$'\t' read -r name license path; do
    if [ "$license" != "$expected" ]; then
      echo "::error file=${path#"$PWD/"}::crate $name declares license '$license', expected '$expected'"
      fail=1
    fi
  done < <(cargo metadata --no-deps --format-version 1 --manifest-path "$manifest" |
    jq -r '.packages[] | [.name, (.license // "none"), .manifest_path] | @tsv')
done < <(git ls-files '*Cargo.toml' 'Cargo.toml')

while IFS= read -r pkg; do
  license=$(jq -r '.license // "none"' "$pkg")
  if [ "$license" != "$expected" ]; then
    echo "::error file=$pkg::package $(jq -r .name "$pkg") declares license '$license', expected '$expected'"
    fail=1
  fi
done < <(git ls-files '*package.json' 'package.json')

if [ "$fail" -ne 0 ]; then
  exit 1
fi
echo "every crate and package declares $expected"
