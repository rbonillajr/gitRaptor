#!/usr/bin/env bash
# Suite 06 — Cost per invocation (ADR-GRD-002 § 5). No daemon exists yet, so
# this measures what the hook layer adds by itself:
#   V0  no hooks
#   V1  ADR design: sh dispatcher, exits in sh for every state but `prepared`,
#       and execs a native binary (lib/hookstub.rs, `rustc -O`) for `prepared`
#   V2  sh dispatcher that always execs the native binary (no sh fast path)
#   V3  guard prototype in pure sh (lib/guard-hook.sh)
# for: update-ref (1 transaction), commit (full dispatcher set), push with
# the governed pre-push evaluation (merge-base), status (post-index-change)
# and a fetch of 1000 refs. Prints p50/p95 in ms per command and the delta
# per hook invocation against V0.
. "$(dirname "$0")/../lib/common.sh"

N="${BENCH_N:-200}"
OUT="$RESULTS_DIR/06-cost.tsv"
# BENCH_ONLY=<regex> runs only the matching scenarios and appends to the TSV.
[ -z "${BENCH_ONLY:-}" ] && tsv scenario variant hook_spawns n p50_ms p95_ms max_ms failures > "$OUT"

new_sandbox
STUB="$SANDBOX/raptor-stub"
rustc -O -o "$STUB" "$LIB/hookstub.rs" 2>/dev/null || { echo "rustc not available" >&2; exit 2; }

# variant_hooks <repo> <variant> <names...>
variant_hooks() {
  local repo="$1" v="$2" d h; shift 2
  d="$(common_dir "$repo")/gitraptor/hooks"
  case "$(common_dir "$repo")" in "$SANDBOX"/*) rm -r "$d" 2>/dev/null ;; esac
  git -C "$repo" config --unset core.hooksPath 2>/dev/null
  [ "$v" = V0 ] && return
  mkdir -p "$d"
  for h in "$@"; do
    case "$v" in
      V1) if [ "$h" = reference-transaction ]; then
            printf '#!/bin/sh\n[ "$1" = prepared ] || exit 0\nexec %s %s "$@"\n' "'$STUB'" "'$h'" > "$d/$h"
          else printf '#!/bin/sh\nexec %s %s "$@"\n' "'$STUB'" "'$h'" > "$d/$h"; fi ;;
      V2) printf '#!/bin/sh\nexec %s %s "$@"\n' "'$STUB'" "'$h'" > "$d/$h" ;;
      V3) sed -e "s#@HOOK@#$h#g" -e "s#@BASE@#main#g" -e "s#@PREV@#$(common_dir "$repo")/hooks#g" \
              -e "s#@COMMON@#$(common_dir "$repo")#g" -e "s#@PRUNE@#1#g" "$LIB/guard-hook.sh" > "$d/$h" ;;
    esac
    chmod +x "$d/$h"
  done
  git -C "$repo" config core.hooksPath "$d"
}

# spawns <repo> <cmd> <setup> <names...>: hook invocations of one run, counted
# with a minimal dispatcher (the fingerprinting probe is O(refs) per call)
spawns() {
  local repo="$1" cmd="$2" setup="$3" d h; shift 3
  d="$(common_dir "$repo")/gitraptor/hooks"; mkdir -p "$d"
  for h in "$@"; do printf '#!/bin/sh\necho x >> %s\ncat >/dev/null\n' "'$SANDBOX/spawns'" > "$d/$h"; chmod +x "$d/$h"; done
  git -C "$repo" config core.hooksPath "$d"
  [ "$setup" != - ] && (cd "$repo" && sh -c "$setup") >/dev/null 2>&1
  : > "$SANDBOX/spawns"; (cd "$repo" && sh -c "$cmd") >/dev/null 2>&1
  rm -r "$(common_dir "$repo")/gitraptor"; git -C "$repo" config --unset core.hooksPath
  wc -l < "$SANDBOX/spawns" | tr -d ' '
}

bench() {
  local scen="$1" repo="$2" setup="$3" cmd="$4" n="$5"; shift 5
  [ -n "${BENCH_ONLY:-}" ] && ! [[ "$scen" =~ $BENCH_ONLY ]] && return
  local sp; sp="$(spawns "$repo" "$cmd" "$setup" "$@")"
  for v in V0 V1 V2 V3; do
    variant_hooks "$repo" "$v" "$@"
    local r; r="$(python3 "$LIB/bench.py" "$n" "$repo" "$setup" "$cmd")"
    tsv "$scen" "$v" "$sp" "$r" >> "$OUT"
    printf '%-26s %s spawns=%-4s %s\n' "$scen" "$v" "$sp" "$r" >&2
  done
  variant_hooks "$repo" V0
}

ALL="$(echo $HOOK_NAMES | tr ' ' '\n' | grep -v -x -e push-to-checkout -e proc-receive -e post-index-change | paste -sd' ' -)"

# Floor: what one process spawn costs on this machine (sh, native binary)
[ -z "${BENCH_ONLY:-}" ] && for c in ":" "/bin/sh -c :" "'$STUB' </dev/null" "/bin/sh -c \"exec '$STUB'\" </dev/null"; do
  r="$(python3 "$LIB/bench.py" "$N" "$SANDBOX" - "$c")"
  c="${c//$SANDBOX/\$SANDBOX}"
  tsv "spawn-floor" "$c" - "$r" >> "$OUT"; printf '%-26s %-40s %s\n' spawn-floor "$c" "$r" >&2
done

init_repo "$SANDBOX/w"; W="$SANDBOX/w"
OID="$(git -C "$W" rev-parse HEAD)"
bench update-ref-1-tx "$W" "git update-ref -d refs/heads/x 2>/dev/null; true" "git update-ref refs/heads/x $OID" "$N" reference-transaction
bench commit-full-set "$W" - "git commit -q --allow-empty -m x" "$N" $ALL
bench status-post-index-change "$W" "touch a.txt" "git status --porcelain" "$N" post-index-change

git init -q --bare "$SANDBOX/r.git"; (cd "$W" && git remote add origin "$SANDBOX/r.git" && git -c core.hooksPath=/dev/null push -q origin main)
bench push-ff-governed "$W" "git -c core.hooksPath=/dev/null commit -q --allow-empty -m p" "git push -q origin main" "$(( N / 2 ))" pre-push reference-transaction

# fetch of 1000 refs into a fresh clone (setup recreates the clone untimed)
(cd "$W" && for i in $(seq 1 1000); do echo "create refs/heads/b$i $OID"; done | git update-ref --stdin && git -c core.hooksPath=/dev/null push -q origin 'refs/heads/*:refs/heads/*')
git init -q "$SANDBOX/f"; git -C "$SANDBOX/f" remote add origin "$SANDBOX/r.git"
F_SETUP="git -c core.hooksPath=/dev/null symbolic-ref --delete refs/remotes/origin/HEAD 2>/dev/null; git for-each-ref --format='delete %(refname)' refs/remotes | git -c core.hooksPath=/dev/null update-ref --stdin"
bench fetch-1000-refs "$SANDBOX/f" "$F_SETUP" "git fetch -q origin" "${BENCH_FETCH_N:-3}" reference-transaction
bench fetch-1000-refs-atomic "$SANDBOX/f" "$F_SETUP" "git fetch -q --atomic origin" "${BENCH_FETCH_N:-3}" reference-transaction
# Without auto-maintenance: since Git 2.5x the fetch itself is batched and the
# per-ref transactions come from the automatic pack-refs prune
bench fetch-1000-refs-no-auto-maint "$SANDBOX/f" "$F_SETUP" "git -c maintenance.auto=false -c gc.auto=0 fetch -q origin" "${BENCH_FETCH_N:-3}" reference-transaction

# pack-refs of 1000 loose branches (refs/heads: governed): one prune
# transaction per ref, what gc --auto / maintenance do in repos with many branches
P_SETUP="git for-each-ref --format='delete %(refname)' 'refs/heads/p*' | git -c core.hooksPath=/dev/null update-ref --stdin; for i in \$(seq 1 1000); do echo \"create refs/heads/p\$i $OID\"; done | git -c core.hooksPath=/dev/null update-ref --stdin"
bench pack-refs-1000-loose "$W" "$P_SETUP" "git pack-refs --all" "${BENCH_FETCH_N:-3}" reference-transaction

drop_sandbox
echo "wrote $OUT" >&2
