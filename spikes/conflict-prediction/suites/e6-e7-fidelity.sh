#!/usr/bin/env bash
# E6 — Fidelity, E7 — hunks, H6 — prefilter.
# R-SYN (the matrix cases, mk-syn.sh) and R-REAL (the last N two-parent merges of git/git,
# default 2000): A2 against the reference `git merge-tree --write-tree` run in a disposable
# clone with isolated config and the same rename limit (P4, P7), for each Git version.
# P4 check: on a sample of R-REAL, `git merge` in a working tree gives the same set as
# `merge-tree`.
#   e6-e7-fidelity.sh [N=2000] [sample=100]
set -euo pipefail
source "$(dirname "$0")/lib/common.sh"
N="${1:-2000}" SAMPLE="${2:-100}"
OUT="$SPIKE_DIR/results/e6-e7-fidelity.txt"
mkdir -p "$SPIKE_DIR/results" "$CKP_ROOT/scratch"
: > "$OUT"
log() { echo "$@" | tee -a "$OUT"; }

SYN="$CKP_ROOT/syn"
[ -d "$SYN/repo" ] || bash "$SPIKE_DIR/suites/lib/mk-syn.sh" "$SYN/repo"
REAL="$CKP_ROOT/git.git"
[ -d "$REAL" ] || git clone -q --bare https://github.com/git/git.git "$REAL"
W="$CKP_ROOT/e6"
mkdir -p "$W"
git --git-dir="$REAL" rev-list --min-parents=2 --max-parents=2 -n "$N" master \
  | while read -r m; do git --git-dir="$REAL" rev-parse "$m^1" "$m^2" | paste - -; done > "$W/real-pairs.tsv"

log "# E6/E7 — $(date -u +%FT%TZ) — load $(load)"
for corpus in syn real; do
  if [ "$corpus" = syn ]; then repo="$SYN/repo" pairs="$SYN/syn-pairs.tsv"; else repo="$REAL" pairs="$W/real-pairs.tsv"; fi
  "$CKP" batch a2 "$repo" "$pairs" --hunks > "$W/$corpus-a2.tsv"
  "$CKP" prefilter "$repo" "$pairs" > "$W/$corpus-pre.tsv"
  while read -r gv gb; do
    "$CKP" batch ref "$repo" "$pairs" --hunks --git "$gb" --scratch "$W/scratch-$corpus-$gv" > "$W/$corpus-ref-$gv.tsv"
    log ""
    log "## corpus=$corpus ($(wc -l < "$pairs" | tr -d ' ') pairs) reference=git $gv"
    python3 "$SPIKE_DIR/suites/lib/compare.py" "$W/$corpus-a2.tsv" "$W/$corpus-ref-$gv.tsv" \
      --labels "$pairs" --prefilter "$W/$corpus-pre.tsv" --diffs 40 | tee -a "$OUT"
  done < <(git_versions)
done

# Markers of the default size: a line that imitates a marker breaks the parse (E7).
"$CKP" batch a2 "$SYN/repo" <(grep mimic "$SYN/syn-pairs.tsv") --hunks --marker 7 > "$W/mimic-7.tsv"
log ""
log "## E7 mimic-markers with marker size 7: hunks=$(cut -f8 "$W/mimic-7.tsv") (size 32: $(grep mimic "$W/syn-a2.tsv" | cut -f8))"

# P4: `git merge` in a working tree against `merge-tree`, on a sample of R-REAL that keeps
# every conflicted pair of the sample first.
WT="$W/p4-wt"
[ -d "$WT" ] || git clone -q --shared --no-checkout "$REAL" "$WT"
{ awk -F'\t' '$5>0' "$W/real-ref-2.50.1-apple.tsv"; awk -F'\t' '$5==0' "$W/real-ref-2.50.1-apple.tsv"; } \
  | head -n "$SAMPLE" | cut -f1,2 > "$W/p4-pairs.tsv" || true   # head closes the pipe early
: > "$W/p4-merge.tsv"
while IFS=$'\t' read -r a b; do
  git -C "$WT" checkout -q -f --detach "$a"
  git -C "$WT" -c merge.renameLimit=1000 -c merge.conflictStyle=merge merge -q --no-edit --no-ff "$b" >/dev/null 2>&1 || true
  files=$(git -C "$WT" diff --name-only --diff-filter=U | sort -u | tr '\n' '|' | sed 's/|$//')
  n=$(echo -n "$files" | awk -F'|' '{print (length($0) ? NF : 0)}')
  printf '%s\t%s\tok\t0\t%s\t%s\t\t[]\t\n' "$a" "$b" "$n" "$files" >> "$W/p4-merge.tsv"
  git -C "$WT" merge --abort >/dev/null 2>&1 || git -C "$WT" reset -q --hard
done < "$W/p4-pairs.tsv"
log ""
log "## P4: git merge (working tree) vs merge-tree, $(wc -l < "$W/p4-pairs.tsv" | tr -d ' ') pairs of R-REAL"
python3 "$SPIKE_DIR/suites/lib/compare.py" "$W/p4-merge.tsv" "$W/real-ref-2.50.1-apple.tsv" --diffs 20 | tee -a "$OUT"
