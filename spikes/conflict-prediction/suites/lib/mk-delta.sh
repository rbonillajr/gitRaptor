#!/usr/bin/env bash
# E8 per-pair time by delta: in a fleet repo, branches delta-<d>-a and delta-<d>-b from main,
# each changing d text files (d = 1, 10, 100, 1000). Half of the files of b are also changed
# by a (different lines: overlap without conflict), and one file has the same line changed on
# both sides (one real conflict). Plumbing only: no checkout. Writes <fleet>/delta-pairs.tsv.
#   mk-delta.sh <fleet>
set -euo pipefail
source "$(dirname "$0")/common.sh"
F="$1" R="$1/repo"
git -C "$R" ls-files | grep -E '\.(rs|py|ts|md|toml|json)$' > "$F/delta-files.txt"
: > "$F/delta-pairs.tsv"
edit() { # <side> <line> <path> -> new blob id
  git -C "$R" cat-file blob "main:$3" | awk -v s="$1" -v n="$2" 'NR==n{print "// delta " s; next} {print}' \
    | git -C "$R" hash-object -w --stdin
}
for d in 1 10 100 1000; do
  for side in a b; do
    export GIT_INDEX_FILE="$F/delta-$d-$side.index"
    git -C "$R" read-tree main
    if [ "$side" = a ]; then sel=$(sed -n "1,${d}p" "$F/delta-files.txt"); line=2
    else sel=$( { sed -n "1,$(( (d + 1) / 2 ))p" "$F/delta-files.txt"; sed -n "$((d + 1)),$((d + d / 2))p" "$F/delta-files.txt"; } ); line=8; fi
    first=1
    for p in $sel; do
      l=$line; [ "$first" = 1 ] && l=4   # the first file: same line on both sides
      first=0
      git -C "$R" update-index --cacheinfo "100644,$(edit "$side$d" "$l" "$p"),$p"
    done
    tree=$(git -C "$R" write-tree)
    c=$(git -C "$R" commit-tree "$tree" -p main -m "delta $d $side")
    git -C "$R" update-ref "refs/heads/delta-$d-$side" "$c"
    unset GIT_INDEX_FILE
    rm -f "$F/delta-$d-$side.index"
  done
  printf 'delta-%s-a\tdelta-%s-b\n' "$d" "$d" >> "$F/delta-pairs.tsv"
done
cat "$F/delta-pairs.tsv"
