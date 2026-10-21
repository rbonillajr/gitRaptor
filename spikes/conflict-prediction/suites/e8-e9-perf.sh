#!/usr/bin/env bash
# E8 — performance and E9 — cost (in-process part) on a fresh R-H fleet (10 worktrees):
# per-pair time by delta, recompute after a commit (with/without prefilter, 1 and 2 jobs),
# initial 55 pairs, and a burst of 10 agents committing every 2–10 s. A2 and A2-W.
#   e8-e9-perf.sh <label> [samples=200] [burst-seconds=600]
set -euo pipefail
source "$(dirname "$0")/lib/common.sh"
LABEL="${1:?label, e.g. indicative or final}" SAMPLES="${2:-200}" BURST="${3:-600}"
OUT="$SPIKE_DIR/results/e8-e9-perf-$LABEL.txt"
mkdir -p "$SPIKE_DIR/results"
F="$CKP_ROOT/perf-$LABEL"
[ -d "$F" ] && { echo "$F exists: remove it first" >&2; exit 1; }
REF="$CKP_ROOT/ref-H"
[ -f "$REF/.git/HEAD" ] || "$CKP" gen-h "$REF"
bash "$SPIKE_DIR/suites/lib/mk-fleet.sh" "$REF" "$F" 10 3 >/dev/null
bash "$SPIKE_DIR/suites/lib/mk-delta.sh" "$F" >/dev/null
{
  echo "# E8/E9 $LABEL — $(date -u +%FT%TZ) — $(sysctl -n machdep.cpu.brand_string), $(sysctl -n hw.ncpu) cores, $(( $(sysctl -n hw.memsize) / 1073741824 )) GB"
  echo "# load at start: $(load)"
} | tee "$OUT"
b() { "$CKP" bench "$@" | tee -a "$OUT"; }
for e in a2 a2w; do
  b pairs "$F" --engine "$e" --samples "$SAMPLES" --pairs "$F/delta-pairs.tsv"
  b recompute "$F" --engine "$e" --samples "$SAMPLES"
done
b recompute "$F" --engine a2 --samples "$SAMPLES" --prefilter
b recompute "$F" --engine a2 --samples "$SAMPLES" --jobs 2
b initial "$F" --engine a2 --samples $(( SAMPLES / 4 ))
b initial "$F" --engine a2w --samples $(( SAMPLES / 4 ))
b burst "$F" --engine a2 --duration "$BURST"
b burst "$F" --engine a2w --duration "$BURST"
b overlap - --samples "$SAMPLES" --burst-files 10000
echo "# load at end: $(load)" | tee -a "$OUT"
