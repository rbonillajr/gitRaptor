#!/usr/bin/env bash
# A fleet over R-H: a fresh clone of the reference repo with N worktrees (default 10), one
# branch `agent-<i>` per worktree with K commits (default 3). Agents i and i+1 edit the same
# line of a shared file (a real conflict); every agent also edits files of its own.
# Writes `<dst>/pairs.tsv` with the N*(N-1)/2 pairs between agents plus the N pairs agent/main.
#   mk-fleet.sh <ref-repo> <dst> [N] [K] [--shared]
set -euo pipefail
source "$(dirname "$0")/common.sh"
SRC="$1" DST="$2" N="${3:-10}" K="${4:-3}" MODE="${5:-}"
[ -e "$DST" ] && { echo "$DST exists" >&2; exit 1; }
mkdir -p "$DST"
if [ "$MODE" = "--shared" ]; then
  git clone -q --shared "$SRC" "$DST/repo"   # the clone borrows R-H through objects/info/alternates
else
  git clone -q --no-local "$SRC" "$DST/repo"
fi
R="$DST/repo"
git -C "$R" config core.autocrlf false
git -C "$R" ls-files | grep -E '\.(rs|py|ts|md|toml|json)$' > "$DST/files.txt"
NF=$(wc -l < "$DST/files.txt" | tr -d ' ')
pick() { sed -n "$(( ($1 % NF) + 1 ))p" "$DST/files.txt"; }
for i in $(seq 0 $((N - 1))); do
  wt="$DST/wt-$i"
  git -C "$R" worktree add -q -b "agent-$i" "$wt" main
  for k in $(seq 1 "$K"); do
    # Shared with the previous agent: same file, same line.
    shared=$(pick $((i * 7919)))
    [ "$k" = 1 ] && [ "$i" -gt 0 ] && shared=$(pick $(((i - 1) * 7919)))
    own=$(pick $((i * 104729 + k * 31337)))
    for f in "$shared" "$own"; do
      awk -v a="agent-$i" -v k="$k" 'NR==3{print "// " a " edit " k; next} {print}' "$wt/$f" > "$wt/$f.tmp" && mv "$wt/$f.tmp" "$wt/$f"
    done
    git -C "$wt" commit -qam "agent-$i commit $k"
  done
done
: > "$DST/pairs.tsv"
for i in $(seq 0 $((N - 1))); do
  printf 'agent-%s\tmain\n' "$i" >> "$DST/pairs.tsv"
  j=$((i + 1))
  while [ "$j" -lt "$N" ]; do printf 'agent-%s\tagent-%s\n' "$i" "$j" >> "$DST/pairs.tsv"; j=$((j + 1)); done
done
echo "fleet: $N worktrees, $(wc -l < "$DST/pairs.tsv" | tr -d ' ') pairs in $DST"
