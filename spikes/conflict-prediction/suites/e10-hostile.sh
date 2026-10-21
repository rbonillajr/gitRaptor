#!/usr/bin/env bash
# E10 — Bounds (M-06) on R-HOS: does each hostile pair end within 2 s and under the memory cap?
#   A2 without bounds (control), A2 with prior bounds only (blob-size cap read from the
#   header, fixed rename limit, cap on the bytes written to the object memory, fail-fast), A2
#   with a deadline checked on every object access (P6), and A2-W (2 s timer, RLIMIT_CPU,
#   resident-memory watch, kill and relaunch).
#   e10-hostile.sh [samples=20]
set -euo pipefail
source "$(dirname "$0")/lib/common.sh"
SAMPLES="${1:-20}"
H="$CKP_ROOT/hostile/repo"
if [ ! -d "$H" ]; then
  git init -q -b main "$H"
  python3 "$SPIKE_DIR/suites/lib/mk-hostile.py" | git -C "$H" fast-import --quiet
fi
printf 'big-blob/ours\tbig-blob/theirs\nmany/ours\tmany/theirs\ndeep/ours\tdeep/theirs\nrenames/ours\trenames/theirs\ndense/ours\tdense/theirs\n' > "$CKP_ROOT/hostile/pairs.tsv"
OUT="$SPIKE_DIR/results/e10-hostile.txt"
mkdir -p "$SPIKE_DIR/results"
echo "# E10 — $(date -u +%FT%TZ) — samples=$SAMPLES" > "$OUT"
run() { # <label> <args...>
  echo "## $1" | tee -a "$OUT"
  shift
  "$CKP" bench hostile "$H" --pairs "$CKP_ROOT/hostile/pairs.tsv" --samples "$SAMPLES" "$@" | tee -a "$OUT"
}
NOCAP=(--max-blob 1000000000000 --rename-limit 100000 --max-written 1000000000000)
CAPS=(--max-blob 8388608 --rename-limit 1000 --max-written 67108864)
run "A2 without bounds (control)" "${NOCAP[@]}"
run "A2 prior bounds: blob cap 8 MiB (header), rename-limit 1000, 64 MiB written" "${CAPS[@]}"
run "A2 prior bounds + fail-fast" "${CAPS[@]}" --fail-fast
run "A2 deadline 2 s on object access, no other bound" "${NOCAP[@]}" --deadline-ms 2000
run "A2 prior bounds + deadline 2 s" "${CAPS[@]}" --deadline-ms 2000
run "A2-W timer 2 s, RSS cap 512 MiB, no other bound" --engine a2w "${NOCAP[@]}" --timeout-ms 2000 --max-rss 536870912
run "A2-W timer 2 s, RSS cap 512 MiB, prior bounds" --engine a2w "${CAPS[@]}" --timeout-ms 2000 --max-rss 536870912
