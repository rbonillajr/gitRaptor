#!/usr/bin/env bash
# E1 — Repo intact (eliminatory). INF-GRP-001 fingerprint (path, kind, size, content, mtime,
# mode, inode, ctime of every file and directory) of the fleet — common dir, .git/worktrees/*
# and every working tree — before and after each engine run. Two fleets over R-H: a full clone
# and a clone with its own `objects/info/alternates` towards R-H (whose objects are also
# fingerprinted). Scenarios: the 55 initial pairs, recompute after a commit in one worktree,
# recompute after the base moves, and a pair that the user already merged elsewhere (the merge
# commit's trees and blobs already exist in the repo, so writing them is a "freshen", F11).
# The user's commits happen outside the measured window.
#   e1-intact.sh [runs=3]
set -euo pipefail
source "$(dirname "$0")/lib/common.sh"
RUNS="${1:-3}"
REF="$CKP_ROOT/ref-H"
[ -f "$REF/.git/HEAD" ] || "$CKP" gen-h "$REF"
OUT="$SPIKE_DIR/results/e1-intact.tsv"
LOG="$SPIKE_DIR/results/e1-intact.log"
mkdir -p "$SPIKE_DIR/results"
printf 'run\tfleet\tengine\tgit\tscenario\tpairs\tchanges\tload\n' > "$OUT"
: > "$LOG"

step() { # <run> <fleet> <engine> <gitver> <gitbin> <scenario> <pairs-file> <dir> [extra scope]
  local run=$1 fleet=$2 engine=$3 gv=$4 gb=$5 sc=$6 pairs=$7 dir=$8 extra=${9:-}
  local store="$CKP_ROOT/profile/e1-$run-$fleet-$gv.git"
  local keep=()
  [ -n "$extra" ] && keep=(--keep-mtime-of "$extra")
  local res
  res=$("$CKP" intact "$dir" "${keep[@]}" -- "$CKP" batch "$engine" "$dir/repo" "$pairs" \
        --git "$gb" --store "$store" --scratch "$CKP_ROOT/scratch" || true)
  local n
  n=$(echo "$res" | head -1 | sed 's/.*changes=//')
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$run" "$fleet" "$engine" "$gv" "$sc" \
    "$(wc -l < "$pairs" | tr -d ' ')" "$n" "$(load)" | tee -a "$OUT"
  { echo "== run=$run fleet=$fleet engine=$engine git=$gv scenario=$sc"; echo "$res"; } >> "$LOG"
}

mkdir -p "$CKP_ROOT/profile" "$CKP_ROOT/scratch"
for run in $(seq 1 "$RUNS"); do
  for fleet in full shared; do
    D="$CKP_ROOT/e1-$run-$fleet"
    [ -d "$D" ] && { echo "$D exists: remove it first" >&2; exit 1; }
    if [ "$fleet" = shared ]; then
      bash "$SPIKE_DIR/suites/lib/mk-fleet.sh" "$REF" "$D" 10 3 --shared >/dev/null
      EXTRA="$REF/.git/objects"
    else
      bash "$SPIKE_DIR/suites/lib/mk-fleet.sh" "$REF" "$D" 10 3 >/dev/null
      EXTRA=""
    fi
    engines=("a2 - -" "a2w - -")
    while read -r gv gb; do engines+=("b $gv $gb"); done < <(git_versions)
    k=0
    for e in "${engines[@]}"; do
      set -- $e
      engine=$1 gv=$2 gb=$3
      [ "$gb" = - ] && gb=/usr/bin/git
      step "$run" "$fleet" "$engine" "$gv" "$gb" initial "$D/pairs.tsv" "$D" "$EXTRA"
      # A user's commit in agent-3's worktree, then the 10 pairs it affects.
      echo "user $engine $gv" >> "$D/wt-3/USER.txt"
      git -C "$D/wt-3" add USER.txt && git -C "$D/wt-3" commit -qm "user commit"
      grep -E '(^|\t)agent-3(\t|$)' "$D/pairs.tsv" > "$D/pairs-a3.tsv"
      step "$run" "$fleet" "$engine" "$gv" "$gb" after-commit "$D/pairs-a3.tsv" "$D" "$EXTRA"
      # The base moves: a commit on main, then the 10 agent/main pairs.
      echo "base $engine $gv" >> "$D/repo/BASE.txt"
      git -C "$D/repo" add BASE.txt && git -C "$D/repo" commit -qm "base moves"
      grep -E '\tmain$' "$D/pairs.tsv" > "$D/pairs-main.tsv"
      step "$run" "$fleet" "$engine" "$gv" "$gb" base-moved "$D/pairs-main.tsv" "$D" "$EXTRA"
      # Already merged: the user merges agent-(k+5) into agent-k (clean), then the engine
      # predicts the pair as it was before that merge.
      pre=$(git -C "$D/wt-$k" rev-parse HEAD)
      git -C "$D/wt-$k" merge -q --no-edit "agent-$((k + 5))"
      printf '%s\tagent-%s\n' "$pre" "$((k + 5))" > "$D/pairs-merged.tsv"
      step "$run" "$fleet" "$engine" "$gv" "$gb" already-merged "$D/pairs-merged.tsv" "$D" "$EXTRA"
      k=$((k + 1))
    done
  done
done
echo "E1 done: $OUT"
